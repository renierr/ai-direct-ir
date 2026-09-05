//! Expanding a root WAT source into one assemblable text: `;; @include`
//! fragments, the `;; @wasi` boundary, and the `;; @data` region.

use wasmtime::Result;

use crate::fail;

use super::data::{append_data_globals, place_data_segments, set_data_region};
use super::scan::scan_module;

/// One expanded WAT source plus the origin of every emitted line, so parser and
/// validator errors can name the file the author actually wrote.
pub(super) struct Expanded {
    pub(super) text: String,
    /// `origins[i]` is the (file, 1-based line) that produced expanded line `i`.
    pub(super) origins: Vec<(std::path::PathBuf, usize)>,
    /// Every transitively included fragment, for rebuild staleness checks.
    pub(super) includes: Vec<std::path::PathBuf>,
    /// The `;; @wasi` directive, if the source has one. A second one would
    /// redefine `$mem-mod` and every lowered function.
    boundary: Option<Directive>,
    /// The address range a `;; @data` directive set aside for segments the
    /// author did not place.
    pub(super) region: Option<DataRegion>,
}

/// A `;; @wasi` directive: what it asked for, and where to put the boundary it
/// generates. The boundary is spliced in only after the whole source is
/// expanded, because what it declares depends on what the modules below it
/// import.
struct Directive {
    /// Parsed at the directive's own line, so a misspelled capability is
    /// reported there rather than wherever the generated text lands.
    boundary: crate::boundary::Boundary,
    file: std::path::PathBuf,
    line: usize,
    /// Index in `origins` where the generated lines go — just past the
    /// directive's own comment line.
    at: usize,
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
    expand_boundary(&mut expanded)?;
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
            record_wasi(args, line, file, index + 1, expanded)?;
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

/// Note a `;; @wasi ...` line and hold its place. The directive itself stays in
/// the expanded text as an ordinary comment, so the origin map keeps its
/// one-to-one shape; the boundary lands right after it in `expand_boundary`.
fn record_wasi(
    args: &str,
    source_line: &str,
    file: &std::path::Path,
    line: usize,
    expanded: &mut Expanded,
) -> Result<()> {
    if let Some(first) = &expanded.boundary {
        return fail(format!(
            "`{}:{line}` generates a second WASI boundary; \
             `{}:{}` already generated one",
            file.display(),
            first.file.display(),
            first.line
        ));
    }
    let boundary =
        crate::boundary::parse(args).map_err(|error| directive_error(file, line, &error))?;
    expanded.text.push_str(source_line);
    expanded.text.push('\n');
    expanded.origins.push((file.to_path_buf(), line));
    expanded.boundary = Some(Directive {
        boundary,
        file: file.to_path_buf(),
        line,
        at: expanded.origins.len(),
    });
    Ok(())
}

/// Generate the WASI 0.2 boundary and splice it in where the directive was.
///
/// This runs after the whole source is expanded because the boundary is not a
/// function of the directive alone: `wasi:filesystem` declares 29 functions and
/// an application names the few it calls, as `(import "fs" ...)` lines that may
/// live in any included fragment; `wasi:sockets` is larger still. Every generated line reports the directive as
/// its origin, so a validator complaint about the boundary points at the line
/// the author actually wrote.
fn expand_boundary(expanded: &mut Expanded) -> Result<()> {
    let Some(directive) = &expanded.boundary else {
        return Ok(());
    };
    let (file, line, at) = (directive.file.clone(), directive.line, directive.at);
    let text = crate::boundary::emit(&directive.boundary, &guest_imports(&expanded.text))
        .map_err(|error| directive_error(&file, line, &error))?;
    splice(expanded, at, &text, (file, line));
    Ok(())
}

/// What the expanded source imports, grouped by import module. The `"fs"` and
/// `"net"` groups are what `;; @wasi filesystem` and `;; @wasi sockets`
/// generate their boundaries for.
fn guest_imports(text: &str) -> crate::boundary::Imports {
    let mut imports = crate::boundary::Imports::new();
    for (module, name) in scan_module(text).imports {
        imports.entry(module).or_default().insert(name);
    }
    imports
}

/// Insert `generated` before expanded line index `at`, crediting every inserted
/// line to `origin`.
fn splice(
    expanded: &mut Expanded,
    at: usize,
    generated: &str,
    origin: (std::path::PathBuf, usize),
) {
    let mut text = String::with_capacity(expanded.text.len() + generated.len());
    let mut origins = Vec::with_capacity(expanded.origins.len() + generated.lines().count());
    let mut inserted = false;
    for (index, (line, source)) in expanded.text.lines().zip(&expanded.origins).enumerate() {
        if index == at {
            push_lines(&mut text, &mut origins, generated, &origin);
            inserted = true;
        }
        text.push_str(line);
        text.push('\n');
        origins.push(source.clone());
    }
    if !inserted {
        push_lines(&mut text, &mut origins, generated, &origin);
    }
    expanded.text = text;
    expanded.origins = origins;
}

fn push_lines(
    text: &mut String,
    origins: &mut Vec<(std::path::PathBuf, usize)>,
    generated: &str,
    origin: &(std::path::PathBuf, usize),
) {
    for line in generated.lines() {
        text.push_str(line);
        text.push('\n');
        origins.push(origin.clone());
    }
}

fn directive_error(
    file: &std::path::Path,
    line: usize,
    error: &wasmtime::Error,
) -> wasmtime::Error {
    wasmtime::Error::msg(format!("{}:{line}: {error}", file.display()))
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
