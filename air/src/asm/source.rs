//! Expanding a root WAT source into one assemblable text: `;; @include`
//! fragments, the `;; @wasi` boundary, and the `;; @data` region.

use wasmtime::Result;

use crate::fail;

use super::data::{append_data_globals, place_data_segments, set_data_region};

/// One expanded WAT source plus the origin of every emitted line, so parser and
/// validator errors can name the file the author actually wrote.
pub(super) struct Expanded {
    pub(super) text: String,
    /// `origins[i]` is the (file, 1-based line) that produced expanded line `i`.
    pub(super) origins: Vec<(std::path::PathBuf, usize)>,
    /// Every transitively included fragment, for rebuild staleness checks.
    pub(super) includes: Vec<std::path::PathBuf>,
    /// Where a `;; @wasi` directive already generated a boundary, if anywhere.
    /// A second one would redefine `$mem-mod` and every lowered function.
    pub(super) boundary: Option<(std::path::PathBuf, usize)>,
    /// The address range a `;; @data` directive set aside for segments the
    /// author did not place.
    pub(super) region: Option<DataRegion>,
}

/// The span `;; @data <start>[..<end>]` gives the harness to place segments in.
/// The author still owns the memory map; they hand over one range of it rather
/// than one address per string.
pub(super) struct DataRegion {
    pub(super) start: u32,
    /// Exclusive upper bound. Without one, packing past the region is only
    /// caught when it collides with a placed segment or overruns the memory.
    pub(super) end: Option<u32>,
    pub(super) file: std::path::PathBuf,
    pub(super) line: usize,
}

impl Expanded {
    /// Keep a reported line inside the map. A parser error at end of input
    /// points one line past the last emitted line; the last line is the
    /// honest answer there.
    pub(super) fn clamp(&self, line: usize) -> usize {
        line.clamp(1, self.origins.len().max(1))
    }

    /// Map a 1-based line of the expanded text back to its source file.
    pub(super) fn origin(&self, line: usize) -> (std::path::PathBuf, usize) {
        self.origins
            .get(line.wrapping_sub(1))
            .cloned()
            .unwrap_or_else(|| (std::path::PathBuf::from("<expanded>"), line))
    }

    pub(super) fn line_text(&self, line: usize) -> &str {
        self.text.lines().nth(line.wrapping_sub(1)).unwrap_or("")
    }
}

/// Expand ordered project-local WAT fragments. A source can contain a
/// standalone `;; @include relative/path.wat` line; the fragment is inserted at
/// that line, may itself include further fragments, and the result is still one
/// ordinary Core WASM module. Every include path is relative to the directory
/// of the root source, so a nested fragment reads exactly like a top-level one.
pub(super) fn expand_wat(root: &std::path::Path) -> Result<Expanded> {
    let mut expanded = Expanded {
        text: String::new(),
        origins: Vec::new(),
        includes: Vec::new(),
        boundary: None,
        region: None,
    };
    let project = root
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let mut open = Vec::new();
    expand_into(root, &project, &mut expanded, &mut open)?;
    place_data_segments(&mut expanded)?;
    append_data_globals(&mut expanded)?;
    Ok(expanded)
}

fn expand_into(
    file: &std::path::Path,
    project: &std::path::Path,
    expanded: &mut Expanded,
    open: &mut Vec<std::path::PathBuf>,
) -> Result<()> {
    let key = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    if open.contains(&key) {
        return fail(format!(
            "WAT include cycle: `{}` is already being expanded",
            file.display()
        ));
    }
    open.push(key);
    let source = std::fs::read_to_string(file)?;
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(path) = trimmed.strip_prefix(";; @include ") {
            let fragment = include_path(project, path.trim())?;
            expanded.includes.push(fragment.clone());
            expand_into(&fragment, project, expanded, open)?;
            continue;
        }
        if let Some(args) = directive_args(trimmed, ";; @wasi") {
            expand_wasi(args, file, index + 1, expanded)?;
            continue;
        }
        if let Some(args) = directive_args(trimmed, ";; @data") {
            set_data_region(args, file, index + 1, expanded)?;
            // The directive is a comment, so it stays in the expanded text and
            // the origin map keeps its one-to-one shape.
        }
        expanded.text.push_str(line);
        expanded.text.push('\n');
        expanded.origins.push((file.to_path_buf(), index + 1));
    }
    open.pop();
    Ok(())
}

/// The argument list of a standalone comment directive, or `None` when the line
/// is not that directive. `;; @wasi` and `;; @wasi stdout` both match;
/// `;; @wasix` does not.
fn directive_args<'a>(line: &'a str, directive: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(directive)?;
    if rest.is_empty() {
        return Some(rest);
    }
    rest.starts_with(char::is_whitespace).then(|| rest.trim())
}

/// Replace a `;; @wasi ...` line with the WASI 0.2 component boundary it names.
/// Every generated line reports the directive as its origin, so a validator
/// complaint about the boundary points at the line the author actually wrote.
fn expand_wasi(
    args: &str,
    file: &std::path::Path,
    line: usize,
    expanded: &mut Expanded,
) -> Result<()> {
    if let Some((first, first_line)) = &expanded.boundary {
        return fail(format!(
            "`{}:{line}` generates a second WASI boundary; \
             `{}:{first_line}` already generated one",
            file.display(),
            first.display()
        ));
    }
    let boundary = crate::boundary::parse(args)
        .map_err(|error| wasmtime::Error::msg(format!("{}:{line}: {error}", file.display())))?;
    let text = crate::boundary::emit(&boundary)
        .map_err(|error| wasmtime::Error::msg(format!("{}:{line}: {error}", file.display())))?;
    for generated in text.lines() {
        expanded.text.push_str(generated);
        expanded.text.push('\n');
        expanded.origins.push((file.to_path_buf(), line));
    }
    expanded.boundary = Some((file.to_path_buf(), line));
    Ok(())
}

fn include_path(project: &std::path::Path, path: &str) -> Result<std::path::PathBuf> {
    let relative = std::path::Path::new(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return fail(format!(
            "WAT include `{path}` must be project-local and relative"
        ));
    }
    let fragment = project.join(relative);
    if !fragment.is_file() {
        return fail(format!(
            "WAT include `{}` is not a file",
            fragment.display()
        ));
    }
    Ok(fragment)
}
