//! Subcommands: build / dist / run / serve / check / inspect / init / help.

use wasmtime::{Engine, ExternType, FuncType, Result, ValType};

use crossterm::{
    execute,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{IsTerminal, Write};
use wasmtime_wasi::I32Exit;

use crate::link::link_all;
use crate::manifest::{Manifest, Target};

pub fn print_help() {
    let mut out = std::io::stdout();
    let color = out.is_terminal();
    if color {
        let _ = execute!(
            out,
            SetForegroundColor(Color::Cyan),
            SetAttribute(Attribute::Bold),
            Print("air"),
            SetAttribute(Attribute::Reset),
            ResetColor,
            Print(format!(
                " {} -- build, validate, and run native, browser, GUI, or component WASM projects\n",
                env!("CARGO_PKG_VERSION")
            )),
        );
    } else {
        let _ = writeln!(
            out,
            "air {} -- build, validate, and run native, browser, GUI, or component WASM projects",
            env!("CARGO_PKG_VERSION")
        );
    }
    help_section(&mut out, color, "USAGE");
    let _ = writeln!(
        out,
        "  air [command] [args]      use host.toml in a project directory or pass a path"
    );
    help_section(&mut out, color, "COMMANDS");
    for (command, description) in [
        (
            "build [manifest.toml]",
            "assemble app.source into app.path; defaults to host.toml",
        ),
        (
            "dist [manifest.toml]",
            "create a self-contained dist/ bundle; defaults to host.toml",
        ),
        (
            "run [manifest.toml]",
            "link and execute a native, GUI, or component app; defaults to host.toml",
        ),
        (
            "serve [manifest.toml]",
            "serve a browser app on localhost; defaults to host.toml",
        ),
        ("<manifest.toml>", "shorthand for `run`"),
        (
            "check [manifest.toml]",
            "link everything, verify wiring; defaults to host.toml",
        ),
        (
            "inspect <module.wasm>",
            "show imports/exports for a prebuilt module",
        ),
        (
            "init <app.wasm>",
            "write a non-overwriting host.toml stub beside an app",
        ),
        (
            "new <name>",
            "choose component, native, browser, or GUI; scaffold a project",
        ),
        ("help, -h, --help", "this text"),
        ("version, -V, --version", "version"),
    ] {
        help_line(&mut out, color, command, description);
    }
    help_section(&mut out, color, "EXAMPLES");
    for (command, description) in [
        ("air new myapp", "choose native, browser, GUI, or component"),
        ("cd myapp && air build", ""),
        ("air check", "validate host.toml and the compiled app"),
        ("air dist", "create a shippable dist/ bundle"),
        ("air run", "native or GUI project"),
        ("air serve", "browser project"),
        (
            "air inspect external-lib.wasm",
            "inspect a prebuilt module's ABI",
        ),
        (
            "air init existing-app.wasm",
            "write a host.toml stub beside it",
        ),
    ] {
        help_line(&mut out, color, command, description);
    }
}

fn help_section(out: &mut std::io::Stdout, color: bool, title: &str) {
    let _ = writeln!(out);
    if color {
        let _ = execute!(
            out,
            SetForegroundColor(Color::Yellow),
            SetAttribute(Attribute::Bold),
            Print(title),
            SetAttribute(Attribute::Reset),
            ResetColor,
            Print(":\n"),
        );
    } else {
        let _ = writeln!(out, "{title}:");
    }
}

fn help_line(out: &mut std::io::Stdout, color: bool, command: &str, description: &str) {
    if color {
        let _ = execute!(
            out,
            Print("  "),
            SetForegroundColor(Color::Green),
            Print(format!("{command:<34}")),
            ResetColor,
            SetForegroundColor(Color::DarkGrey),
            Print(description),
            ResetColor,
            Print("\n"),
        );
    } else {
        let _ = writeln!(out, "  {command:<34}{description}");
    }
}

/// Directory a manifest's relative paths resolve against.
fn manifest_base(manifest_path: &str) -> std::path::PathBuf {
    std::path::Path::new(manifest_path)
        .parent()
        .map(|p| {
            if p.as_os_str().is_empty() {
                std::path::PathBuf::from(".")
            } else {
                p.to_path_buf()
            }
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

pub fn run_manifest(engine: &Engine, path: &str) -> Result<()> {
    let manifest: Manifest = crate::manifest::load(path)?;
    build_if_needed(engine, path, &manifest)?;
    if manifest.target == Target::Browser {
        return fail(format!(
            "{path} targets a browser; build it, then serve its directory and open index.html"
        ));
    }
    if manifest.target == Target::Gui {
        return crate::gui::run(engine.clone(), manifest, manifest_base(path));
    }
    if manifest.target == Target::Component {
        return crate::component::run(engine, &manifest, &manifest_base(path));
    }
    let base = manifest_base(path);
    if manifest.worker_count() > 1 {
        return run_workers(engine, path, &base);
    }
    let mut linked = link_all(engine, &manifest, &base)?;
    if linked.is_server {
        let run = linked
            .app_inst
            .get_typed_func::<i32, i32>(&mut linked.store, &linked.run_name)?;
        println!(
            "serving {:?} on 127.0.0.1:{} (Ctrl-C to stop)",
            manifest.root, linked.port
        );
        run.call(&mut linked.store, linked.port as i32)?;
        Ok(())
    } else {
        let func = linked
            .app_inst
            .get_func(&mut linked.store, &linked.run_name)
            .ok_or_else(|| wasmtime::Error::msg(format!("app has no func {}", linked.run_name)))?;
        match func.call(&mut linked.store, &[], &mut []) {
            Ok(_) => Ok(()),
            Err(e) => {
                // WASI proc_exit surfaces as I32Exit: exit code, not crash.
                if let Some(exit) = e.downcast_ref::<I32Exit>() {
                    std::process::exit(exit.0);
                }
                Err(e)
            }
        }
    }
}

/// Assemble and validate a manifest-declared WAT source without spawning WABT.
pub fn cmd_build(engine: &Engine, path: &str) -> Result<()> {
    let manifest = crate::manifest::load(path)?;
    build_wat(engine, path, &manifest)
}

/// Assemble only when the declared source is newer than its output, or when the
/// output is missing. This keeps run/check/dist usable as single commands while
/// preserving `air build` as the explicit force-rebuild command.
fn build_if_needed(engine: &Engine, path: &str, manifest: &Manifest) -> Result<()> {
    let base = manifest_base(path);
    build_providers(engine, path, manifest, &base, false)?;
    let Some(source) = &manifest.app.source else {
        return Ok(());
    };
    let source = manifest_path(&base, source);
    let output = manifest_path(&base, &manifest.app.path);
    if is_stale(&source, &output)? {
        assemble(engine, &source, &output, manifest.declared_target, path)?;
    }
    Ok(())
}

fn build_wat(engine: &Engine, path: &str, manifest: &Manifest) -> Result<()> {
    let source = manifest.app.source.as_ref().ok_or_else(|| {
        wasmtime::Error::msg(format!(
            "{path} has no [app].source; app.path `{}` is prebuilt or has no declared WAT source",
            manifest.app.path
        ))
    })?;
    let base = manifest_base(path);
    build_providers(engine, path, manifest, &base, true)?;
    assemble(
        engine,
        &manifest_path(&base, source),
        &manifest_path(&base, &manifest.app.path),
        manifest.declared_target,
        path,
    )
}

/// Assemble any declared provider sources. A provider artifact is built from
/// its source for the same reason an application's is: otherwise editing the
/// WAT beside it changes nothing.
fn build_providers(
    engine: &Engine,
    path: &str,
    manifest: &Manifest,
    base: &std::path::Path,
    force: bool,
) -> Result<()> {
    for provider in &manifest.providers {
        let Some(source) = &provider.source else {
            continue;
        };
        let source = manifest_path(base, source);
        let output = manifest_path(base, &provider.path);
        if force || is_stale(&source, &output)? {
            assemble(engine, &source, &output, Some(Target::Component), path)?;
        }
    }
    Ok(())
}

/// Assemble one WAT source into one WASM artifact.
fn assemble(
    engine: &Engine,
    source: &std::path::Path,
    output: &std::path::Path,
    declared: Option<Target>,
    manifest_path: &str,
) -> Result<()> {
    let expanded = expand_wat(source)?;
    let wasm = wat::parse_str(&expanded.text)
        .map_err(|e| assemble_error(source, &expanded, &e.to_string()))?;
    // The assembled bytes say what this is; the manifest never has to. When it
    // does say, a disagreement is caught here rather than in the wrong linker.
    let is_component = crate::manifest::is_component_binary(&wasm);
    match (declared, is_component) {
        (Some(Target::Component), false) => {
            return fail(format!(
                "{manifest_path} declares `target = \"component\"` but `{}` assembles to a Core WASM module",
                source.display()
            ));
        }
        (Some(declared), true) if declared != Target::Component => {
            return fail(format!(
                "`{}` assembles to a component, but {manifest_path} declares a Core target",
                source.display()
            ));
        }
        _ => {}
    }
    // Validate before writing: `build` must never leave an artifact behind that
    // `check`, `dist`, or a commit would later pick up as good. Compiling (not
    // just validating) is what reports the offending function index, which is
    // the only handle the include map can translate.
    if is_component {
        wasmtime::component::Component::new(engine, &wasm)
            .map_err(|e| validate_error(source, &expanded, &e))?;
    } else {
        wasmtime::Module::new(engine, &wasm).map_err(|e| validate_error(source, &expanded, &e))?;
    }
    std::fs::write(output, wasm)?;
    // Progress goes to stderr: `run`, `check`, and `dist` may rebuild first,
    // and an app's piped stdout must stay the app's alone.
    eprintln!("built {} -> {}", source.display(), output.display());
    Ok(())
}

/// True when `output` is missing, or no newer than `source` or any fragment it
/// includes. `>=`, not `>`: a source written in the same timestamp tick as the
/// artifact must still rebuild.
fn is_stale(source: &std::path::Path, output: &std::path::Path) -> Result<bool> {
    let output_time = match std::fs::metadata(output) {
        Ok(metadata) => metadata.modified()?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e.into()),
    };
    let mut stale = std::fs::metadata(source)?.modified()? >= output_time;
    for fragment in expand_wat(source)?.includes {
        stale |= std::fs::metadata(fragment)?.modified()? >= output_time;
    }
    Ok(stale)
}

/// One expanded WAT source plus the origin of every emitted line, so parser and
/// validator errors can name the file the author actually wrote.
struct Expanded {
    text: String,
    /// `origins[i]` is the (file, 1-based line) that produced expanded line `i`.
    origins: Vec<(std::path::PathBuf, usize)>,
    /// Every transitively included fragment, for rebuild staleness checks.
    includes: Vec<std::path::PathBuf>,
    /// Where a `;; @wasi` directive already generated a boundary, if anywhere.
    /// A second one would redefine `$mem-mod` and every lowered function.
    boundary: Option<(std::path::PathBuf, usize)>,
}

impl Expanded {
    /// Keep a reported line inside the map. A parser error at end of input
    /// points one line past the last emitted line; the last line is the
    /// honest answer there.
    fn clamp(&self, line: usize) -> usize {
        line.clamp(1, self.origins.len().max(1))
    }

    /// Map a 1-based line of the expanded text back to its source file.
    fn origin(&self, line: usize) -> (std::path::PathBuf, usize) {
        self.origins
            .get(line.wrapping_sub(1))
            .cloned()
            .unwrap_or_else(|| (std::path::PathBuf::from("<expanded>"), line))
    }

    fn line_text(&self, line: usize) -> &str {
        self.text.lines().nth(line.wrapping_sub(1)).unwrap_or("")
    }
}

/// Expand ordered project-local WAT fragments. A source can contain a
/// standalone `;; @include relative/path.wat` line; the fragment is inserted at
/// that line, may itself include further fragments, and the result is still one
/// ordinary Core WASM module. Every include path is relative to the directory
/// of the root source, so a nested fragment reads exactly like a top-level one.
fn expand_wat(root: &std::path::Path) -> Result<Expanded> {
    let mut expanded = Expanded {
        text: String::new(),
        origins: Vec::new(),
        includes: Vec::new(),
        boundary: None,
    };
    let project = root
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    let mut open = Vec::new();
    expand_into(root, &project, &mut expanded, &mut open)?;
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
    let text = crate::boundary::emit(&boundary);
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

/// Report an assembly failure against the authored fragment, not the expanded
/// text the author never sees. Falls back to the raw parser message when the
/// location cannot be recovered.
fn assemble_error(root: &std::path::Path, expanded: &Expanded, message: &str) -> wasmtime::Error {
    let Some((line, col)) = wat_location(message) else {
        return wasmtime::Error::msg(format!(
            "could not assemble `{}`: {message}",
            root.display()
        ));
    };
    let line = expanded.clamp(line);
    let (file, source_line) = expanded.origin(line);
    let reason = message.lines().next().unwrap_or(message);
    wasmtime::Error::msg(format!(
        "could not assemble `{}`: {reason}\n     --> {}:{source_line}:{col}\n      |\n {source_line:4} | {}\n      | {:>col$}",
        root.display(),
        file.display(),
        expanded.line_text(line),
        "^",
    ))
}

/// Recover `line:col` from the `wat` crate's rendered error location.
fn wat_location(message: &str) -> Option<(usize, usize)> {
    let marker = message
        .lines()
        .find_map(|line| line.trim().strip_prefix("--> "))?;
    let mut parts = marker.rsplitn(3, ':');
    let col = parts.next()?.trim().parse().ok()?;
    let line = parts.next()?.trim().parse().ok()?;
    Some((line, col))
}

/// Report a validation failure against the source line that defines the
/// offending function. WASM validation speaks in function indices and byte
/// offsets; the include map is the only thing that can translate that back.
fn validate_error(
    root: &std::path::Path,
    expanded: &Expanded,
    error: &wasmtime::Error,
) -> wasmtime::Error {
    let detail = format!("{error:#}");
    let scan = scan_module(&expanded.text);
    let module = module_index(&detail).unwrap_or(0);
    let located = function_index(&detail)
        .and_then(|index| {
            scan.modules
                .get(module)
                .and_then(|module| module.functions.get(index))
                .copied()
        })
        .map(|line| (expanded.origin(line), expanded.line_text(line).to_string()));
    let Some(((file, source_line), text)) = located else {
        return wasmtime::Error::msg(format!("`{}` failed validation: {detail}", root.display()));
    };
    wasmtime::Error::msg(format!(
        "`{}` failed validation: {detail}\n     --> {}:{source_line}\n      |\n {source_line:4} | {}",
        root.display(),
        file.display(),
        text.trim_end(),
    ))
}

/// Recover the Core module index from a compiler message. A Core app has one
/// module; a component may instantiate several.
fn module_index(detail: &str) -> Option<usize> {
    let rest = detail.split("wasm[").nth(1)?;
    rest.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// Recover a Core WASM function index from a validator or compiler message.
fn function_index(detail: &str) -> Option<usize> {
    let digits = |rest: &str| -> Option<usize> {
        rest.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    };
    if let Some(index) = detail.split("function[").nth(1).and_then(digits) {
        return Some(index);
    }
    detail.split("func ").nth(1).and_then(digits)
}

/// A named data segment, plus the byte range of its whole form.
struct DataSegment {
    name: String,
    line: usize,
    start: usize,
    end: usize,
}

/// One Core WASM module found in the source: a plain `(module ...)` app, or a
/// `(core module ...)` inside a component.
struct ModuleScan {
    start: usize,
    /// Byte index of the paren closing this module.
    close: usize,
    /// Source line of every function in Core index order: imported functions
    /// first, then the ones defined in the body.
    functions: Vec<usize>,
    data: Vec<DataSegment>,
}

/// One pass over a WAT source: enough structure to translate validator output
/// and to generate the address/length globals for named data segments.
/// Comments and string literals are skipped, so `(func` or `(data` inside them
/// never counts.
struct Scan {
    /// Modules in source order, which is also Core module index order.
    modules: Vec<ModuleScan>,
}

/// A form the scanner has entered but not yet left.
struct Frame<'a> {
    head: &'a str,
    name: Option<String>,
    line: usize,
    start: usize,
    imported: Vec<usize>,
    defined: Vec<usize>,
    data: Vec<DataSegment>,
}

impl<'a> Frame<'a> {
    fn new(head: &'a str, name: Option<String>, line: usize, start: usize) -> Self {
        Frame {
            head,
            name,
            line,
            start,
            imported: Vec::new(),
            defined: Vec::new(),
            data: Vec::new(),
        }
    }
}

fn scan_module(text: &str) -> Scan {
    let bytes = text.as_bytes();
    let mut open: Vec<Frame<'_>> = Vec::new();
    let mut scan = Scan {
        modules: Vec::new(),
    };
    let mut line = 1usize;
    let mut block = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if block > 0 {
            if bytes[i] == b'(' && bytes.get(i + 1) == Some(&b';') {
                block += 1;
                i += 2;
            } else if bytes[i] == b';' && bytes.get(i + 1) == Some(&b')') {
                block -= 1;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        match bytes[i] {
            b';' if bytes.get(i + 1) == Some(&b';') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' if bytes.get(i + 1) == Some(&b';') => {
                block = 1;
                i += 2;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        b'\n' => {
                            line += 1;
                            i += 1;
                        }
                        _ => i += 1,
                    }
                }
            }
            b'(' => {
                let start = i;
                i += 1;
                let mut head = &text[i..i + token_len(&bytes[i..])];
                i += head.len();
                // `core module`, `core func`, `core instance`: the second word
                // is what identifies the form.
                if head == "core" {
                    let skipped = leading_space(&bytes[i..]);
                    let after = i + skipped;
                    let len = token_len(&bytes[after..]);
                    if len > 0 {
                        head = &text[after..after + len];
                        line += bytes[i..after].iter().filter(|b| **b == b'\n').count();
                        i = after + len;
                    }
                }
                let depth = open.len();
                if head == "func" {
                    if depth >= 1 && open[depth - 1].head == "module" {
                        open[depth - 1].defined.push(line);
                    } else if depth >= 2
                        && open[depth - 1].head == "import"
                        && open[depth - 2].head == "module"
                    {
                        open[depth - 2].imported.push(line);
                    }
                }
                // Only a named segment directly inside a module opts in.
                let name = if head == "data" && depth >= 1 && open[depth - 1].head == "module" {
                    identifier(text, &bytes[i..], i)
                } else {
                    None
                };
                open.push(Frame::new(head, name, line, start));
            }
            b')' => {
                if let Some(frame) = open.pop() {
                    if let Some(name) = frame.name {
                        if let Some(parent) = open.last_mut() {
                            parent.data.push(DataSegment {
                                name,
                                line: frame.line,
                                start: frame.start,
                                end: i,
                            });
                        }
                    }
                    if frame.head == "module" {
                        let mut functions = frame.imported;
                        functions.extend(frame.defined);
                        scan.modules.push(ModuleScan {
                            start: frame.start,
                            close: i,
                            functions,
                            data: frame.data,
                        });
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    scan.modules.sort_by_key(|module| module.start);
    scan
}

/// Length of the token starting at `bytes[0]`, stopping at any WAT delimiter.
fn token_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')' | b';' | b'"'))
        .unwrap_or(bytes.len())
}

fn leading_space(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len())
}

/// The `$name` immediately following a form's head keyword, if there is one.
fn identifier(text: &str, rest: &[u8], offset: usize) -> Option<String> {
    let skipped = rest.iter().position(|b| !b.is_ascii_whitespace())?;
    if rest[skipped] != b'$' {
        return None;
    }
    let start = offset + skipped;
    let len = token_len(&rest[skipped..]);
    Some(text[start..start + len].to_string())
}

/// Named data segments are the one place authored WAT carries a hand-maintained
/// byte count, and a stale count truncates output without ever failing
/// validation. For every `(data $name (i32.const <addr>) "...")` the harness
/// appends `$name.ptr` and `$name.len` globals, so the author reads the length
/// instead of restating it. Unnamed segments are untouched.
fn append_data_globals(expanded: &mut Expanded) -> Result<()> {
    let scan = scan_module(&expanded.text);
    // Insert from the last module backwards so earlier byte offsets stay valid.
    let mut modules: Vec<&ModuleScan> = scan
        .modules
        .iter()
        .filter(|module| !module.data.is_empty())
        .collect();
    modules.sort_by_key(|module| std::cmp::Reverse(module.close));
    for module in modules {
        let mut generated = String::new();
        let mut origins = Vec::new();
        let mut placed: Vec<(&str, u32, u32)> = Vec::new();
        for segment in &module.data {
            let form = &expanded.text[segment.start..=segment.end];
            let (file, line) = expanded.origin(segment.line);
            let described = format!("{} at {}:{line}", segment.name, file.display());
            let address = data_address(form, &described)?;
            let length = data_length(form, &described)?;
            for (other, other_address, other_length) in &placed {
                if address < other_address + other_length && *other_address < address + length {
                    return fail(format!(
                        "data segment `{}` overlaps `{other}`: {address}..{} vs {other_address}..{}",
                        segment.name,
                        address + length,
                        other_address + other_length
                    ));
                }
            }
            placed.push((&segment.name, address, length));
            generated.push_str(&format!(
                "  (global {name}.ptr i32 (i32.const {address})) (global {name}.len i32 (i32.const {length}))\n",
                name = segment.name,
            ));
            origins.push((file, line));
        }

        // Insert at the closing paren's exact byte, never at a line boundary: a
        // line boundary can fall inside a multi-line form.
        let close = module.close;
        let line_of_close = expanded.text[..close].matches('\n').count() + 1;
        expanded.text = format!(
            "{}\n{generated}{}",
            &expanded.text[..close],
            &expanded.text[close..]
        );
        // The closing paren's line becomes a prefix line, the generated lines,
        // and a remainder line; prefix and remainder keep the original origin.
        let carried = expanded.origin(line_of_close);
        origins.push(carried);
        expanded
            .origins
            .splice(line_of_close..line_of_close, origins);
    }
    Ok(())
}

/// The literal address of a data segment. A named segment must place itself at
/// a constant, because the generated `.ptr` global has to hold that constant.
fn data_address(form: &str, described: &str) -> Result<u32> {
    let Some(rest) = form.split("i32.const").nth(1) else {
        return fail(format!(
            "data segment `{described}` needs a literal `(i32.const <addr>)` offset"
        ));
    };
    let token: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == 'x')
        .filter(|c| *c != '_')
        .collect();
    let parsed = match token.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => token.parse(),
    };
    parsed.map_err(|_| {
        wasmtime::Error::msg(format!(
            "data segment `{described}` has an offset `{token}` that is not a literal i32"
        ))
    })
}

/// Decoded byte length of every string literal in a data segment.
fn data_length(form: &str, described: &str) -> Result<u32> {
    let bytes = form.as_bytes();
    let mut total = 0u32;
    let mut i = 0usize;
    let mut block = 0usize;
    while i < bytes.len() {
        if block > 0 {
            if bytes[i] == b'(' && bytes.get(i + 1) == Some(&b';') {
                block += 1;
                i += 2;
            } else if bytes[i] == b';' && bytes.get(i + 1) == Some(&b')') {
                block -= 1;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        match bytes[i] {
            b';' if bytes.get(i + 1) == Some(&b';') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' if bytes.get(i + 1) == Some(&b';') => {
                block = 1;
                i += 2;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    let (bytes_used, produced) = escape_len(&bytes[i..], described)?;
                    i += bytes_used;
                    total += produced;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    Ok(total)
}

/// How many source bytes one character of a WAT string literal consumes, and
/// how many bytes it contributes to the segment. The escape set is closed: an
/// unrecognised escape is a lex error, so refusing here beats guessing a length.
fn escape_len(rest: &[u8], described: &str) -> Result<(usize, u32)> {
    if rest[0] != b'\\' {
        return Ok((1, 1));
    }
    match rest.get(1) {
        Some(b't' | b'n' | b'r' | b'"' | b'\'' | b'\\') => Ok((2, 1)),
        Some(b'u') => {
            let close = rest
                .iter()
                .position(|b| *b == b'}')
                .ok_or_else(|| unknown_escape(described))?;
            let hex =
                std::str::from_utf8(&rest[3..close]).map_err(|_| unknown_escape(described))?;
            let scalar =
                u32::from_str_radix(hex.trim(), 16).map_err(|_| unknown_escape(described))?;
            let ch = char::from_u32(scalar).ok_or_else(|| unknown_escape(described))?;
            Ok((close + 1, ch.len_utf8() as u32))
        }
        Some(digit) if digit.is_ascii_hexdigit() => {
            if rest.get(2).is_some_and(u8::is_ascii_hexdigit) {
                Ok((3, 1))
            } else {
                Err(unknown_escape(described))
            }
        }
        _ => Err(unknown_escape(described)),
    }
}

fn unknown_escape(described: &str) -> wasmtime::Error {
    wasmtime::Error::msg(format!(
        "data segment `{described}` has an escape this harness cannot measure; \
         a named segment's length must be exact"
    ))
}

/// Create a clean, relocatable distribution beside the manifest. Native
/// bundles include their executable host; browser bundles use browser assets.
pub fn cmd_dist(path: &str) -> Result<()> {
    if path == "host.toml" && !std::path::Path::new(path).is_file() {
        return fail(
            "no host.toml in this directory; run `air dist <manifest.toml>` or change to a project directory"
                .into(),
        );
    }
    let manifest = crate::manifest::load(path)?;
    // A release uses current source when the manifest declares it. Prebuilt
    // modules remain valid: they have no source and are checked as supplied.
    let engine = Engine::default();
    build_if_needed(&engine, path, &manifest)?;
    cmd_check(&engine, path, &manifest)?;
    let base = std::fs::canonicalize(manifest_base(path))?;
    let dist = base.join("dist");
    if dist.exists() {
        std::fs::remove_dir_all(&dist)?;
    }
    std::fs::create_dir(&dist)?;

    let app = copy_bundle_file(&base, &dist, &manifest.app.path)?;
    let mut manifest_out = toml::Table::new();
    manifest_out.insert(
        "target".into(),
        toml::Value::String(
            match manifest.target {
                Target::Native => "native",
                Target::Browser => "browser",
                Target::Gui => "gui",
                Target::Component => "component",
            }
            .into(),
        ),
    );
    manifest_out.insert(
        "mode".into(),
        toml::Value::String(
            match manifest.mode {
                crate::manifest::Mode::Server => "server",
                crate::manifest::Mode::Command => "command",
            }
            .into(),
        ),
    );
    for (key, value) in [
        (
            "port",
            manifest.port.map(|v| toml::Value::Integer(v.into())),
        ),
        (
            "memory_pages",
            manifest
                .memory_pages
                .map(|v| toml::Value::Integer(v.into())),
        ),
        (
            "workers",
            manifest.workers.map(|v| toml::Value::Integer(v as i64)),
        ),
    ] {
        if let Some(value) = value {
            manifest_out.insert(key.into(), value);
        }
    }
    if let Some(root) = &manifest.root {
        let source = manifest_path(&base, root);
        let name = source
            .file_name()
            .ok_or_else(|| wasmtime::Error::msg("root has no directory name"))?;
        let dest = dist.join(name);
        copy_dir(&source, &dest)?;
        manifest_out.insert(
            "root".into(),
            toml::Value::String(name.to_string_lossy().into()),
        );
        if let Some(guest) = &manifest.guest {
            manifest_out.insert("guest".into(), toml::Value::String(guest.clone()));
        }
    }
    if matches!(
        manifest.target,
        Target::Native | Target::Gui | Target::Component
    ) {
        let mut libs = Vec::new();
        for lib in &manifest.libs {
            let path = copy_bundle_file(&base, &dist, &lib.path)?;
            let mut item = toml::Table::new();
            item.insert("path".into(), toml::Value::String(path));
            item.insert("as".into(), toml::Value::String(lib.namespace.clone()));
            libs.push(toml::Value::Table(item));
        }
        if !libs.is_empty() {
            manifest_out.insert("libs".into(), toml::Value::Array(libs));
        }
        let mut bridges = Vec::new();
        for bridge in &manifest.bridges {
            let path = copy_bundle_file(&base, &dist, &bridge.path)?;
            let mut item = toml::Table::new();
            item.insert("path".into(), toml::Value::String(path));
            item.insert("as".into(), toml::Value::String(bridge.namespace.clone()));
            item.insert("alloc".into(), toml::Value::String(bridge.alloc.clone()));
            let calls = bridge
                .calls
                .iter()
                .map(|call| {
                    let mut call_out = toml::Table::new();
                    call_out.insert("as".into(), toml::Value::String(call.name.clone()));
                    call_out.insert("func".into(), toml::Value::String(call.func.clone()));
                    call_out.insert("in_ptr".into(), toml::Value::Integer(call.in_ptr as i64));
                    call_out.insert("in_len".into(), toml::Value::Integer(call.in_len as i64));
                    call_out.insert("out_ptr".into(), toml::Value::Integer(call.out_ptr as i64));
                    call_out.insert("out_len".into(), toml::Value::Integer(call.out_len.into()));
                    call_out.insert("max_in".into(), toml::Value::Integer(call.max_in.into()));
                    toml::Value::Table(call_out)
                })
                .collect();
            item.insert("calls".into(), toml::Value::Array(calls));
            bridges.push(toml::Value::Table(item));
        }
        if !bridges.is_empty() {
            manifest_out.insert("bridges".into(), toml::Value::Array(bridges));
        }
        let executable = std::env::current_exe()?;
        let host_name = if cfg!(windows) { "air.exe" } else { "air" };
        std::fs::copy(executable, dist.join(host_name))?;
    } else {
        for name in ["index.html", "web-host.js"] {
            let source = base.join(name);
            if !source.is_file() {
                return fail(format!(
                    "browser distribution requires `{}`",
                    source.display()
                ));
            }
            std::fs::copy(source, dist.join(name))?;
        }
    }
    let mut app_out = toml::Table::new();
    app_out.insert("path".into(), toml::Value::String(app));
    app_out.insert("run".into(), toml::Value::String(manifest.app.run));
    manifest_out.insert("app".into(), toml::Value::Table(app_out));
    std::fs::write(
        dist.join("host.toml"),
        toml::to_string_pretty(&manifest_out)?,
    )?;
    println!(
        "created {} distribution at {}",
        match manifest.target {
            Target::Native => "native",
            Target::Browser => "browser",
            Target::Gui => "GUI",
            Target::Component => "component",
        },
        dist.display()
    );
    Ok(())
}

/// Copy a manifest-relative file under `dist/` without allowing a source path
/// outside the project to dictate an unsafe destination path.
fn copy_bundle_file(
    base: &std::path::Path,
    dist: &std::path::Path,
    source: &str,
) -> Result<String> {
    let source = std::fs::canonicalize(manifest_path(base, source))?;
    let name = source
        .file_name()
        .ok_or_else(|| wasmtime::Error::msg("module has no file name"))?
        .to_string_lossy()
        .into_owned();
    let dest = dist.join(&name);
    std::fs::copy(&source, &dest)?;
    Ok(name)
}

fn copy_dir(source: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    if !source.is_dir() {
        return fail(format!("`{}` is not a directory", source.display()));
    }
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let child = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &child)?;
        } else {
            std::fs::copy(entry.path(), child)?;
        }
    }
    Ok(())
}

/// Build and distribution paths always belong to their manifest, never to the
/// caller's working directory. Linking keeps its legacy fallback separately.
fn manifest_path(base: &std::path::Path, path: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// Serve a browser app from its manifest directory. Browsers require HTTP to
/// fetch WASM modules, and WebAssembly uses its own content type.
pub fn cmd_serve(path: &str) -> Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let manifest = crate::manifest::load(path)?;
    if manifest.target != Target::Browser {
        return fail(format!(
            "{path} targets native; `air serve` is for browser apps"
        ));
    }
    let base = std::fs::canonicalize(manifest_base(path))?;
    let port = manifest.port.unwrap_or(8000);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| wasmtime::Error::msg(format!("bind 127.0.0.1:{port}: {e}")))?;
    println!(
        "serving {} at http://127.0.0.1:{port}/ (Ctrl-C to stop)",
        base.display()
    );
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("accept: {e}");
                continue;
            }
        };
        let mut request = [0; 4096];
        let n = match stream.read(&mut request) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let request = String::from_utf8_lossy(&request[..n]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");
        let rel = target
            .split('?')
            .next()
            .unwrap_or("/")
            .trim_start_matches('/');
        let candidate = if rel.is_empty() {
            base.join("index.html")
        } else {
            base.join(rel)
        };
        let file = std::fs::canonicalize(&candidate)
            .ok()
            .filter(|file| file.starts_with(&base));
        match file.and_then(|file| std::fs::read(&file).ok().map(|body| (file, body))) {
            Some((file, body)) => {
                let content_type = match file.extension().and_then(|s| s.to_str()) {
                    Some("wasm") => "application/wasm",
                    Some("js") => "text/javascript; charset=utf-8",
                    Some("html") => "text/html; charset=utf-8",
                    Some("css") => "text/css; charset=utf-8",
                    _ => "application/octet-stream",
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
            None => {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        }
    }
    Ok(())
}

fn func_sig(t: &FuncType) -> String {
    let p: Vec<_> = t.params().map(|k| k.to_string()).collect();
    let r: Vec<_> = t.results().map(|k| k.to_string()).collect();
    format!("(func {} -> {})", p.join(" "), r.join(" "))
}

/// Show what a (possibly foreign) module needs and offers — the starting
/// point for wiring any language's .wasm output into a manifest.
pub fn cmd_inspect(engine: &Engine, path: &str) -> Result<()> {
    let m = wasmtime::Module::from_file(engine, path)?;
    println!("module: {path}");
    println!("imports:");
    let mut needs_mem = false;
    let mut namespaces: Vec<String> = vec![];
    for imp in m.imports() {
        let ty = match imp.ty() {
            ExternType::Func(f) => func_sig(&f),
            ExternType::Memory(t) => {
                format!("(memory min={} max={:?})", t.minimum(), t.maximum())
            }
            ExternType::Global(_) => "(global)".into(),
            ExternType::Table(_) => "(table)".into(),
            _ => "(?)".into(),
        };
        println!("  {}.{} : {ty}", imp.module(), imp.name());
        if imp.module() == "env" && imp.name() == "memory" {
            needs_mem = true;
        } else if !namespaces.contains(&imp.module().to_string()) {
            namespaces.push(imp.module().to_string());
        }
    }
    println!("exports:");
    for exp in m.exports() {
        println!("  {}", exp.name());
    }
    println!("needs:");
    println!(
        "  shared env.memory: {}",
        if needs_mem { "yes" } else { "no (own memory)" }
    );
    println!("  namespaces: {}", namespaces.join(", "));
    Ok(())
}

/// Validate a manifest end-to-end without executing: links everything and
/// verifies the entry func signature.
pub fn cmd_check(engine: &Engine, manifest_path: &str, manifest: &Manifest) -> Result<()> {
    build_if_needed(engine, manifest_path, manifest)?;
    let base = manifest_base(manifest_path);
    if manifest.target == Target::Browser {
        return check_browser(engine, manifest_path, manifest, &base);
    }
    if manifest.target == Target::Gui {
        return check_gui(engine, manifest_path, manifest, &base);
    }
    if manifest.target == Target::Component {
        return crate::component::check(engine, manifest_path, manifest, &base);
    }
    let linked = link_all(engine, manifest, &base)?;
    let app_mod =
        wasmtime::Module::from_file(engine, crate::link::join(&base, &manifest.app.path))?;
    let want_server = linked.is_server;
    let found = app_mod.exports().find(|e| e.name() == linked.run_name);
    match found {
        Some(e) => match e.ty() {
            ExternType::Func(f) => {
                let is_i32_i32 = f.params().len() == 1
                    && matches!(f.params().next(), Some(ValType::I32))
                    && f.results().len() == 1
                    && matches!(f.results().next(), Some(ValType::I32));
                let ok = if want_server {
                    is_i32_i32
                } else {
                    f.params().len() == 0 && f.results().len() == 0
                };
                if ok {
                    println!("run `{}`: signature ok", linked.run_name);
                } else {
                    return fail(format!(
                        "run `{}` has {}, want {}",
                        linked.run_name,
                        func_sig(&f),
                        if want_server {
                            "(func i32 -> i32)"
                        } else {
                            "(func  -> )"
                        }
                    ));
                }
            }
            _ => {
                return fail(format!("run `{}` is not a func", linked.run_name));
            }
        },
        None => {
            return fail(format!(
                "app {} has no export `{}`",
                manifest.app.path, linked.run_name
            ));
        }
    }
    println!("check {manifest_path}: all modules linked, all imports satisfied");
    Ok(())
}

fn check_gui(
    engine: &Engine,
    manifest_path: &str,
    manifest: &Manifest,
    base: &std::path::Path,
) -> Result<()> {
    if !matches!(manifest.mode, crate::manifest::Mode::Command) {
        return fail("GUI projects require mode = \"command\"".into());
    }
    let app = wasmtime::Module::from_file(engine, crate::link::join(base, &manifest.app.path))?;
    // The common linker is the authority for every target: a project may add
    // any Core WASM provider through [[libs]] or [[bridges]]. Instantiation
    // proves that providers and built-in host capabilities satisfy imports.
    link_all(engine, manifest, base)?;
    match app.exports().find(|e| e.name() == manifest.app.run) {
        Some(e) if browser_func_sig_ok(&e.ty(), 0, 0) => {}
        _ => {
            return fail(format!(
                "GUI app needs zero-argument export `{}`",
                manifest.app.run
            ));
        }
    }
    println!("run `{}`: signature ok", manifest.app.run);
    println!("check {manifest_path}: all modules linked, all imports satisfied");
    Ok(())
}

/// Browser modules are checked against the browser host ABI, not linked in
/// Wasmtime: browser imports are implemented by the generated JavaScript host.
fn check_browser(
    engine: &Engine,
    manifest_path: &str,
    manifest: &Manifest,
    base: &std::path::Path,
) -> Result<()> {
    if !matches!(manifest.mode, crate::manifest::Mode::Command) {
        return fail("browser projects require mode = \"command\"".into());
    }
    if !manifest.libs.is_empty() || !manifest.bridges.is_empty() {
        return fail("browser projects do not support [[libs]] or [[bridges]] yet".into());
    }
    let app_path = crate::link::join(base, &manifest.app.path);
    let app = wasmtime::Module::from_file(engine, &app_path)?;
    let mut needs_frame = false;
    for import in app.imports() {
        if import.module() != "web" {
            return fail(format!(
                "browser app import {}.{} is unsupported; browser projects may import only web.*",
                import.module(),
                import.name()
            ));
        }
        if !browser_import_sig_ok(import.name(), &import.ty()) {
            return fail(format!(
                "browser app import web.{} has an unsupported type or is not provided by web-host.js",
                import.name()
            ));
        }
        needs_frame |= import.name() == "request_frame";
    }
    let entry = app.exports().find(|e| e.name() == manifest.app.run);
    match entry {
        Some(e) if browser_func_sig_ok(&e.ty(), 0, 0) => {}
        Some(e) => {
            return fail(format!(
                "run `{}` must be a zero-argument function, found {:?}",
                manifest.app.run,
                e.ty()
            ));
        }
        None => {
            return fail(format!(
                "app {} has no export `{}`",
                manifest.app.path, manifest.app.run
            ));
        }
    }
    if needs_frame
        && !app
            .exports()
            .any(|e| e.name() == "frame" && browser_func_sig_ok(&e.ty(), 0, 0))
    {
        return fail("web.request_frame requires an exported frame() function".into());
    }
    println!("run `{}`: signature ok", manifest.app.run);
    println!("check {manifest_path}: browser imports are provided by web-host.js");
    Ok(())
}

fn browser_func_sig_ok(ty: &ExternType, params: usize, results: usize) -> bool {
    matches!(ty, ExternType::Func(f) if f.params().len() == params && f.results().len() == results)
}

fn browser_import_sig_ok(name: &str, ty: &ExternType) -> bool {
    match name {
        "canvas_width" | "canvas_height" | "mouse_x" | "mouse_y" => browser_func_sig_ok(ty, 0, 1),
        "request_frame" => browser_func_sig_ok(ty, 0, 0),
        "key_down" => browser_func_sig_ok(ty, 1, 1),
        "clear" => browser_func_sig_ok(ty, 4, 0),
        "fill_rect" => browser_func_sig_ok(ty, 8, 0),
        _ => false,
    }
}

fn fail<T>(msg: String) -> Result<T> {
    Err(wasmtime::Error::msg(msg))
}

/// Server with host-owned accept loop: the main thread accepts, N worker
/// threads each own a fully linked instance and run `handle(cfd)` per
/// connection. Blocking sockets + OS threads, std only: no async runtime,
/// no new deps. One worker dying (trap) costs its connection, not the server.
fn run_workers(engine: &Engine, path: &str, base: &std::path::Path) -> Result<()> {
    use crate::host::Sock;
    use std::sync::mpsc;

    let manifest: Manifest = crate::manifest::load(path)?;
    let port = manifest.port.unwrap_or(8123);
    let n = manifest.worker_count();
    let entry = manifest.app.run.clone();
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| wasmtime::Error::msg(format!("bind 127.0.0.1:{port}: {e}")))?;
    println!(
        "serving {:?} on 127.0.0.1:{port} with {n} workers (Ctrl-C to stop)",
        manifest.root
    );
    let mut txs = Vec::new();
    for w in 0..n {
        let (tx, rx) = mpsc::channel::<std::net::TcpStream>();
        txs.push(tx);
        let eng = engine.clone();
        let mpath = path.to_string();
        let bdir = base.to_path_buf();
        let func = entry.clone();
        std::thread::Builder::new()
            .name(format!("worker-{w}"))
            .spawn(move || {
                let m: Manifest = match crate::manifest::load(&mpath) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("worker-{w}: manifest: {e}");
                        return;
                    }
                };
                let mut linked = match link_all(&eng, &m, &bdir) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("worker-{w}: link: {e}");
                        return;
                    }
                };
                let handle = match linked
                    .app_inst
                    .get_typed_func::<i32, i32>(&mut linked.store, &func)
                {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("worker-{w}: entry `{func}`: {e}");
                        return;
                    }
                };
                for stream in rx {
                    let h = linked.store.data_mut().alloc_sock(Sock::Conn(stream));
                    if let Err(e) = handle.call(&mut linked.store, h) {
                        eprintln!("worker-{w}: handle: {e}");
                    }
                    // Host closes: the handle is dropped here either way,
                    // so a leaky app can't exhaust the socket table.
                    linked.store.data_mut().socks.remove(&h);
                }
            })
            .map_err(|e| wasmtime::Error::msg(format!("spawn worker: {e}")))?;
    }
    let mut i = 0usize;
    for conn in listener.incoming() {
        match conn {
            Ok(c) => {
                let _ = txs[i % n].send(c);
                i += 1;
            }
            Err(e) => eprintln!("accept: {e}"),
        }
    }
    Ok(())
}

/// Scaffold a full project dir plus AI-facing intent, architecture, and test docs.
/// Templates are baked into the binary (include_str!), so a fresh project
/// carries harness instructions and rules with it. Never overwrites.
pub fn cmd_new(engine: &Engine, name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return fail(format!("bad project name `{name}`: use [A-Za-z0-9_-]"));
    }
    let target = prompt_target()?;
    let dir = std::path::Path::new(name);
    if dir.exists() {
        let empty = std::fs::read_dir(dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !empty {
            return fail(format!("`{name}` exists and is not empty, refusing"));
        }
    } else {
        std::fs::create_dir_all(dir)?;
    }
    let wat = dir.join(format!("{name}.wat"));
    let toml = dir.join("host.toml");
    let readme = dir.join("README.md");
    let agents = dir.join("AGENTS.md");
    let gitignore = dir.join(".gitignore");
    let index = dir.join("index.html");
    let web_host = dir.join("web-host.js");
    let docs = dir.join("docs");
    let src = dir.join("src");
    let state = src.join("state.wat");
    let src_readme = src.join("README.md");
    let skills = dir.join(".agents").join("skills").join("ai-direct-ir");
    let skill = skills.join("SKILL.md");
    let spec = docs.join("01-spec.md");
    let architecture = docs.join("02-architecture.md");
    let verification = docs.join("03-verification.md");
    let mut files = vec![
        &wat,
        &state,
        &toml,
        &readme,
        &agents,
        &gitignore,
        &src_readme,
        &skill,
    ];
    if target == Target::Browser {
        files.extend([&index, &web_host]);
    }
    for p in files {
        if p.exists() {
            return fail(format!("`{}` exists, refusing to overwrite", p.display()));
        }
    }
    for p in [&spec, &architecture, &verification] {
        if p.exists() {
            return fail(format!("`{}` exists, refusing to overwrite", p.display()));
        }
    }
    let hello = format!("hello from {name}\n");
    let (starter, manifest) = match target {
        Target::Browser => browser_starter(name),
        Target::Gui => gui_starter(name),
        Target::Component => component_starter(name),
        Target::Native => {
            let starter = format!(
                ";; {name}.wat — {name} app, hosted by air.\n\
         ;; Build: air build\n\
         ;; Check: air check\n\
         ;; Run:   air run\n\
         ;;\n\
         ;; Command-mode contract: own memory (export it for WASI),\n\
         ;; WASI stdio, `_start` entry, `proc_exit` code is the exit code.\n\
         ;; Need sockets, shared libs, or bridges? New needs go in the\n\
         ;; manifest (TOML), never in harness code.\n\
         ;;\n\
         ;; A named data segment gets $name.ptr and $name.len from air.\n\
         ;; Never write a string length by hand: it silently goes stale.\n\
         \n\
         (module\n\
         \x20 (import \"wasi_snapshot_preview1\" \"fd_write\"\n\
         \x20   (func $fd_write (param i32 i32 i32 i32) (result i32)))\n\
         \x20 (import \"wasi_snapshot_preview1\" \"proc_exit\"\n\
         \x20   (func $exit (param i32)))\n\
         \x20 (memory 1)\n\
         \x20 (export \"memory\" (memory 0))\n\
         \n\
         \x20 (func (export \"_start\")\n\
         \x20   (i32.store (i32.const 0) (global.get $hello.ptr))\n\
         \x20   (i32.store (i32.const 4) (global.get $hello.len))\n\
         \x20   (call $fd_write (i32.const 1) (i32.const 0)\n\
         \x20     (i32.const 1) (i32.const 8))\n\
         \x20   (drop)\n\
         \x20   (call $exit (i32.const 0))\n\
         \x20   (unreachable))\n\
         \n\
         \x20 (data $hello (i32.const 0x1000) \"{hello}\")\n\
         )\n",
                name = name,
                // WAT string syntax needs an escaped newline, not a literal one.
                hello = hello.escape_default()
            );
            let manifest = format!(
                "# {name}: command-mode app. Build, check, then run:\n\
         #   air build && air check && air run\n\
         mode = \"command\"\n\
         \n\
         [app]\n\
         source = \"{name}.wat\"\n\
         path = \"{name}.wasm\"\n\
         run = \"_start\"\n"
            );
            (starter, manifest)
        }
    };
    std::fs::create_dir_all(&src)?;
    std::fs::write(
        &state,
        ";; Application state and state-transition helpers belong here.\n",
    )?;
    std::fs::write(&wat, starter)?;
    std::fs::write(&toml, manifest)?;
    std::fs::write(&gitignore, include_str!("../templates/project-gitignore"))?;
    std::fs::write(
        &readme,
        project_doc(
            include_str!("../templates/project-readme.md"),
            name,
            &target,
        ),
    )?;
    std::fs::create_dir_all(&docs)?;
    std::fs::write(
        &src_readme,
        project_doc(
            include_str!("../templates/project-src-readme.md"),
            name,
            &target,
        ),
    )?;
    std::fs::create_dir_all(&skills)?;
    std::fs::write(
        &skill,
        project_doc(include_str!("../templates/project-skill.md"), name, &target),
    )?;
    std::fs::write(
        &spec,
        project_doc(include_str!("../templates/project-spec.md"), name, &target),
    )?;
    std::fs::write(
        &architecture,
        project_doc(
            include_str!("../templates/project-architecture.md"),
            name,
            &target,
        ),
    )?;
    std::fs::write(
        &verification,
        project_doc(
            include_str!("../templates/project-verification.md"),
            name,
            &target,
        ),
    )?;
    std::fs::write(
        &agents,
        project_doc(
            include_str!("../templates/project-agents.md"),
            name,
            &target,
        ),
    )?;
    if target == Target::Browser {
        std::fs::write(&index, include_str!("../templates/browser-index.html"))?;
        std::fs::write(
            &web_host,
            include_str!("../templates/browser-host.js").replace("__APPNAME__", name),
        )?;
    }
    let manifest: Manifest = crate::manifest::load(toml.to_str().unwrap())?;
    build_wat(engine, toml.to_str().unwrap(), &manifest)?;
    let extra = if target == Target::Browser {
        "\n  index.html\n  web-host.js"
    } else {
        ""
    };
    println!(
        "created {name}/:\n  {name}.wat\n  {name}.wasm\n  host.toml\n  README.md\n  AGENTS.md\n  docs/\n  src/\n  .agents/skills/ai-direct-ir/\n  .gitignore{extra}\n\
         next:\n  cd {name} && air check{}",
        if target == Target::Browser {
            " && air serve"
        } else {
            " && air run"
        }
    );
    Ok(())
}

/// Render one application-focused document for the selected target; generated
/// projects should not carry irrelevant instructions for the other runtime.
fn project_doc(template: &str, name: &str, target: &Target) -> String {
    let (target_name, run_command, workflow, files, contract, verify, agent_contract) = if *target
        == Target::Browser
    {
        (
            "browser",
            "air serve",
            "`air serve` hosts this directory at a localhost URL with the required\nWASM MIME type. Open that URL in a browser. `air run` is not used for\nbrowser projects. `air dist` contains `index.html`, `web-host.js`, and the\ncompiled application; deploy that directory to any static web host.",
            "| `index.html` | The page containing the application canvas. |\n| `web-host.js` | Trusted browser runtime that implements the `web.*` imports. |",
            "The module exports `start()` (the `[app].run` entry). It may import only the\ndeclared `web.*` functions implemented in `web-host.js`: Canvas dimensions,\n`clear`, `fill_rect`, keyboard state, pointer coordinates, and frame scheduling.\nIf it imports `request_frame()`, it must export `frame()`. `web-host.js` owns\nbrowser events and drawing effects; WAT owns application state and behavior.",
            "Use `air serve` and test the result in a browser",
            "- `web-host.js` is trusted application runtime, not generated glue to discard.\n  Keep its imports and the WAT imports in lockstep.\n- Do not import WASI, `term.*`, `net.*`, `[[libs]]`, or `[[bridges]]`: those are\n  native-target capabilities and browser validation rejects them.\n- Keep rendering explicit through `web.*`; do not add arbitrary JavaScript\n  evaluation or DOM object handles as shortcuts.",
        )
    } else if *target == Target::Gui {
        (
            "native GUI",
            "air run",
            "`air run` opens the native egui window and calls the configured entry once per UI frame. `air dist` contains the executable, manifest, and compiled application.",
            "",
            "The module exports a zero-argument frame function. It may import built-in capabilities such as `ui.*` and any project-declared `[[libs]]` or `[[bridges]]` provider. WAT owns state; the host renders the built-in controls using egui. Read `docs/PROJECT.md` before changing a built-in import.",
            "Run `air run`, interact with the window, and confirm expected state changes",
            "- `ui.label(ptr, len)` and `ui.button(ptr, len) -> i32` are built-in GUI conveniences, not a limit on application dependencies. Add Core WASM providers through `[[libs]]` or `[[bridges]]`; their exports can use any namespace.
- The entry runs once per UI frame. Button clicks are returned on the following frame; retain application state in WAT globals or memory.
- `air check` links the complete declared graph. An unresolved import is an integration error, not a reason to add an application-specific harness API.",
        )
    } else {
        (
            "native",
            "air run",
            "`air run` executes the configured entry through the native host. It is\nnot a browser application and has no DOM or Canvas runtime. `air dist`\ncontains the `air` executable, a rewritten local `host.toml`, the app,\ndeclared WASM dependencies, and any configured `root` data directory.",
            "",
            "Command applications normally export `_start()` and can use declared WASI\nstdio/files and optional `term.*` terminal calls. Server applications use\n`mode = \"server\"` and an entry such as `run(port)` or `handle(cfd)` with\n`workers = N`. Only imports implemented by the native host and configured in\n`host.toml` are available.",
            "Run `air run` and exercise the expected CLI or server behavior",
            "- `mode = \"command\"`: keep a plain stdio path; use `term.*` only after\n  checking `term.available` so pipes and CI still work.\n- `mode = \"server\"`: document socket and buffer ownership. Use `workers = N`\n  only when the entry handles one accepted connection.\n- Browser `web.*` imports, `index.html`, and `web-host.js` do not exist for this\n  target. A browser UI is a separate browser project.",
        )
    };
    template
        .replace("__APPNAME__", name)
        .replace("__TARGET_NAME__", target_name)
        .replace("__RUN_COMMAND__", run_command)
        .replace("__TARGET_WORKFLOW__", workflow)
        .replace("__TARGET_FILES__", files)
        .replace("__TARGET_CONTRACT__", contract)
        .replace("__VERIFY_ACTION__", verify)
        .replace("__TARGET_AGENT_CONTRACT__", agent_contract)
}

fn prompt_target() -> Result<Target> {
    use std::io::{self, IsTerminal, Write};
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        return select_target();
    }
    print!("target [component/native/browser/gui] (component): ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    match input.trim() {
        "" | "component" => Ok(Target::Component),
        "native" => Ok(Target::Native),
        "browser" => Ok(Target::Browser),
        "gui" => Ok(Target::Gui),
        other => fail(format!(
            "unknown target `{other}`: choose component, native, browser, or gui"
        )),
    }
}

/// A compact selector keeps interactive `new` discoverable without giving up
/// the line-input fallback needed by piped scripts and CI.
fn select_target() -> Result<Target> {
    use crossterm::{
        cursor::{MoveToColumn, MoveUp},
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        style::{Attribute, Print, SetAttribute},
        terminal::{disable_raw_mode, enable_raw_mode},
    };
    use std::io::{self, Write};

    struct RawMode;
    impl Drop for RawMode {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
        }
    }

    fn draw(selected: usize) -> std::io::Result<()> {
        let mut out = io::stdout();
        execute!(
            out,
            MoveToColumn(0),
            Print("Create target (Up/Down, Enter):\r\n")
        )?;
        for (index, (name, description)) in [
            (
                "Native",
                "WASI Preview 1, terminal, server, and WASM libraries",
            ),
            ("Browser", "Canvas application served to a web browser"),
            ("GUI", "Native egui desktop application"),
            ("Component", "WASM component on WASI 0.2"),
        ]
        .iter()
        .enumerate()
        {
            let marker = if index == selected { ">" } else { " " };
            if index == selected {
                execute!(out, SetAttribute(Attribute::Bold))?;
            }
            execute!(out, Print(format!(" {marker} {name:<9} {description}\r\n")))?;
            if index == selected {
                execute!(out, SetAttribute(Attribute::Reset))?;
            }
        }
        execute!(out, Print("\r\n"))?;
        out.flush()
    }

    enable_raw_mode().map_err(|e| wasmtime::Error::msg(format!("terminal raw mode: {e}")))?;
    let _raw = RawMode;
    let mut selected = 0;
    draw(selected).map_err(|e| wasmtime::Error::msg(format!("terminal draw: {e}")))?;
    loop {
        let event =
            event::read().map_err(|e| wasmtime::Error::msg(format!("terminal read: {e}")))?;
        let Event::Key(key) = event else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                selected = selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                selected = (selected + 1).min(3);
            }
            KeyCode::Enter => {
                println!();
                return Ok(match selected {
                    0 => Target::Component,
                    1 => Target::Native,
                    2 => Target::Browser,
                    _ => Target::Gui,
                });
            }
            KeyCode::Esc => {
                println!();
                return fail("project creation cancelled".into());
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                println!();
                return fail("project creation cancelled".into());
            }
            _ => continue,
        }
        let mut out = io::stdout();
        execute!(out, MoveUp(6))
            .map_err(|e| wasmtime::Error::msg(format!("terminal redraw: {e}")))?;
        draw(selected).map_err(|e| wasmtime::Error::msg(format!("terminal draw: {e}")))?;
    }
}

fn browser_starter(name: &str) -> (String, String) {
    let starter = format!(
        ";; {name}.wat -- Canvas app hosted by web-host.js.\n\
         ;; Build: air build\n\
         ;; Check: air check\n\
         ;; Run: serve this directory and open index.html.\n\
         ;; web.* is the browser ABI: keep app state in WASM and drawing explicit.\n\
         \n\
         (module\n\
          \x20 (import \"web\" \"clear\" (func $clear (param i32 i32 i32 i32)))\n\
          \x20 (import \"web\" \"fill_rect\" (func $fill_rect (param i32 i32 i32 i32 i32 i32 i32 i32)))\n\
          \x20 ;; @include src/state.wat\n\
          \x20 (func (export \"start\")\n\
         \x20   (call $clear (i32.const 20) (i32.const 24) (i32.const 35) (i32.const 255))\n\
         \x20   (call $fill_rect (i32.const 48) (i32.const 48) (i32.const 320) (i32.const 160)\n\
         \x20     (i32.const 67) (i32.const 151) (i32.const 255) (i32.const 255)))\n\
         )\n"
    );
    let manifest = format!(
        "# {name}: browser Canvas app.\n\
         target = \"browser\"\n\
         mode = \"command\"\n\
         \n\
         [app]\n\
         source = \"{name}.wat\"\n\
         path = \"{name}.wasm\"\n\
         run = \"start\"\n"
    );
    (starter, manifest)
}

/// A WASI 0.2 command component, authored as component WAT. `air` assembles
/// it in-process: the component path needs no bindings generator and no
/// language toolchain, exactly like the Core path.
fn component_starter(name: &str) -> (String, String) {
    let starter = include_str!("../templates/component-starter.wat").replace("__NAME__", name);
    let manifest = format!(
        "# {name}: WASM component on WASI 0.2.\n\
         target = \"component\"\n\
         mode = \"command\"\n\
         \n\
         [app]\n\
         source = \"{name}.wat\"\n\
         path = \"{name}.wasm\"\n\
         run = \"wasi:cli/run\"\n"
    );
    (starter, manifest)
}

fn gui_starter(name: &str) -> (String, String) {
    let starter = format!(
        ";; {name}.wat -- native egui app hosted by air.\n\
         ;; The entry runs every UI frame. Strings are UTF-8 in env.memory.\n\
         ;; A named data segment gets $name.ptr and $name.len from air, so\n\
         ;; no string length is ever written by hand.\n\
         (module\n\
          \x20 (import \"env\" \"memory\" (memory 1))\n\
          \x20 (import \"ui\" \"label\" (func $label (param i32 i32)))\n\
          \x20 (import \"ui\" \"button\" (func $button (param i32 i32) (result i32)))\n\
          \x20 ;; @include src/state.wat\n\
          \x20 (global $count (mut i32) (i32.const 0))\n\
         \x20 (func (export \"frame\")\n\
         \x20   (call $label (global.get $title.ptr) (global.get $title.len))\n\
         \x20   (if (call $button (global.get $increment.ptr) (global.get $increment.len))\n\
         \x20     (then (global.set $count (i32.add (global.get $count) (i32.const 1)))))\n\
         \x20   (call $label (global.get $status.ptr) (global.get $status.len)))\n\
         \x20 (data $title (i32.const 0) \"Hello from {name}\")\n\
         \x20 (data $increment (i32.const 256) \"Increment\")\n\
         \x20 (data $status (i32.const 512) \"Button is ready\")\n\
         )\n"
    );
    let manifest = format!(
        "# {name}: native egui GUI app.\n\
         target = \"gui\"\n\
         mode = \"command\"\n\
         \n\
         [app]\n\
         source = \"{name}.wat\"\n\
         path = \"{name}.wasm\"\n\
         run = \"frame\"\n"
    );
    (starter, manifest)
}

/// Scaffold a manifest stub from an app module's own imports.
pub fn cmd_init(engine: &Engine, app_path: &str) -> Result<()> {
    let m = wasmtime::Module::from_file(engine, app_path)?;
    let mut namespaces: Vec<String> = vec![];
    let mut wants_net = false;
    for imp in m.imports() {
        match imp.module() {
            "env" | "wasi_snapshot_preview1" => {}
            "net" => wants_net = true,
            ns => {
                if !namespaces.contains(&ns.to_string()) {
                    namespaces.push(ns.to_string());
                }
            }
        }
    }
    let stem = std::path::Path::new(app_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app".into());
    let dir = std::path::Path::new(app_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pref = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut out = String::new();
    if wants_net {
        out.push_str("mode = \"server\"\nport = 8123\n");
        out.push_str("# root = \"www\"  # uncomment to preopen a data dir (WASI fd 3)\n");
    } else {
        out.push_str("mode = \"command\"\n");
    }
    out.push_str("# memory_pages = 2  # floor; import minima always win\n\n");
    for ns in &namespaces {
        out.push_str(&format!(
            "[[libs]]  # `{ns}` wanted by {stem}.wasm — point at the providing module\npath = \"<lib-{ns}.wasm>\"\nas = \"{ns}\"\n\n"
        ));
    }
    if namespaces.is_empty() {
        out.push_str("# no custom namespaces: app needs only env/net/wasi\n\n");
    }
    let run = if wants_net { "run" } else { "_start" };
    out.push_str(&format!(
        "[app]\npath = \"{pref}{stem}.wasm\"\nrun = \"{run}\"\n"
    ));
    let toml_path = format!("{pref}host.toml");
    // Never silently overwrite an existing manifest.
    if std::path::Path::new(&toml_path).exists() {
        return Err(wasmtime::Error::msg(format!(
            "{toml_path} exists, refusing to overwrite"
        )));
    }
    std::fs::write(&toml_path, &out)?;
    println!("wrote {toml_path}:\n{out}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{data_address, data_length, scan_module, wat_location};

    #[test]
    fn scanned_functions_follow_core_index_order() {
        let text = "(module
  ;; (func in a comment must not count)
  (import \"wasi\" \"fd_write\"
    (func $write (param i32) (result i32)))
  (type $t (func (param i32)))
  (func $first (result i32) (i32.const 1))
  (data (i32.const 0) \"(func in a string)\")
  (func $second)
)
";
        // Imported functions come first in the index space, then definitions.
        assert_eq!(scan_module(text).modules[0].functions, vec![4, 6, 8]);
    }

    #[test]
    fn scanned_functions_ignore_block_comments() {
        let text = "(module (; (func hidden) ;) (func $only))\n";
        assert_eq!(scan_module(text).modules[0].functions, vec![1]);
    }

    #[test]
    fn scanned_data_segments_are_named_and_bounded() {
        let text = "\
(module
  (data (i32.const 0) \"unnamed stays untouched\")
  (data $banner (i32.const 4096) \"hi\")
)
";
        let scan = scan_module(text);
        let data = &scan.modules[0].data;
        assert_eq!(data.len(), 1, "only a named segment opts in");
        assert_eq!(data[0].name, "$banner");
        assert_eq!(data[0].line, 3);
        let form = &text[data[0].start..=data[0].end];
        assert_eq!(form, "(data $banner (i32.const 4096) \"hi\")");
    }

    #[test]
    fn data_addresses_accept_decimal_and_hex() {
        assert_eq!(
            data_address("(data $a (i32.const 1024) \"x\")", "a").unwrap(),
            1024
        );
        assert_eq!(
            data_address("(data $a (i32.const 0x1000) \"x\")", "a").unwrap(),
            4096
        );
        assert!(data_address("(data $a (global.get $base) \"x\")", "a").is_err());
    }

    #[test]
    fn data_lengths_count_decoded_bytes() {
        let count = |form: &str| data_length(form, "seg").unwrap();
        assert_eq!(count("(data $a (i32.const 0) \"abc\")"), 3);
        // Concatenated literals are one segment.
        assert_eq!(count("(data $a (i32.const 0) \"ab\" \"c\")"), 3);
        // Escapes are one byte each, whatever their source length.
        assert_eq!(count("(data $a (i32.const 0) \"a\\nb\\tc\")"), 5);
        assert_eq!(count("(data $a (i32.const 0) \"\\1b[0m\")"), 4);
        assert_eq!(count("(data $a (i32.const 0) \"\\\"\\\\\")"), 2);
        // Multi-byte characters count as their UTF-8 length, written either way.
        assert_eq!(count("(data $a (i32.const 0) \"\u{25c6}\")"), 3);
        assert_eq!(count("(data $a (i32.const 0) \"\\u{25c6}\")"), 3);
        // A comment inside the form contributes nothing, quotes included.
        assert_eq!(
            count("(data $a (i32.const 0) ;; \"not data\"\n  \"ok\")"),
            2
        );
    }

    #[test]
    fn unmeasurable_escapes_are_refused_rather_than_guessed() {
        assert!(data_length("(data $a (i32.const 0) \"\\q\")", "seg").is_err());
        assert!(data_length("(data $a (i32.const 0) \"\\f\")", "seg").is_err());
    }

    #[test]
    fn wat_location_reads_the_rendered_marker() {
        let message = "expected `)`\n     --> <anon>:12:5\n      |\n";
        assert_eq!(wat_location(message), Some((12, 5)));
    }

    #[test]
    fn wat_location_tolerates_a_message_without_one() {
        assert_eq!(wat_location("something went wrong"), None);
    }
}
