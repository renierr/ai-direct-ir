//! `air build` -- deciding *what* to assemble from a manifest. The assembler
//! itself is `crate::asm`.

use wasmtime::{Engine, Result};

use crate::asm::{assemble, is_stale};
use crate::manifest::{Manifest, Target};

use super::{manifest_base, manifest_path};

/// Assemble and validate a manifest-declared WAT source without spawning WABT.
pub fn cmd_build(engine: &Engine, path: &str) -> Result<()> {
    let manifest = crate::manifest::load(path)?;
    build_wat(engine, path, &manifest)
}

/// Assemble only when the declared source is newer than its output, or when the
/// output is missing. This keeps run/check/dist usable as single commands while
/// preserving `air build` as the explicit force-rebuild command.
pub(super) fn build_if_needed(engine: &Engine, path: &str, manifest: &Manifest) -> Result<()> {
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

pub(super) fn build_wat(engine: &Engine, path: &str, manifest: &Manifest) -> Result<()> {
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
