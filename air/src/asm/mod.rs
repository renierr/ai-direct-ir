//! The WAT assembler: source text in, validated `.wasm` out.
//!
//! `assemble` is the whole pipeline -- expand the source (`source`), place its
//! named data segments (`data`, `scan`), parse and validate, and report any
//! failure against the authored fragment rather than the expanded text
//! (`diag`). The generated boundary it expands lives in `crate::boundary` and
//! `crate::wit`.

mod data;
mod diag;
mod scan;
mod source;

use wasmtime::{Engine, Result};

use crate::fail;
use crate::manifest::Target;

use diag::{assemble_error, validate_error};
use source::expand_wat;

/// Assemble one WAT source into one WASM artifact.
pub(crate) fn assemble(
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
pub(crate) fn is_stale(source: &std::path::Path, output: &std::path::Path) -> Result<bool> {
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
