//! Translating an assembler or validator complaint about the expanded text
//! back to the line the author actually wrote.

use super::scan::scan_module;
use super::source::Expanded;

/// Report an assembly failure against the authored fragment, not the expanded
/// text the author never sees. Falls back to the raw parser message when the
/// location cannot be recovered.
pub(super) fn assemble_error(
    root: &std::path::Path,
    expanded: &Expanded,
    message: &str,
) -> wasmtime::Error {
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
pub(super) fn validate_error(
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

#[cfg(test)]
mod tests {
    use super::wat_location;

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
