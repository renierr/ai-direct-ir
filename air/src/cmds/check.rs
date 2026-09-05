//! `air check` -- link a manifest and verify every import is satisfied.

use wasmtime::{Engine, ExternType, Result, ValType};

use crate::link::link_all;
use crate::manifest::{Manifest, Target};

use crate::fail;

use super::build::build_if_needed;
use super::{func_sig, manifest_base};

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
