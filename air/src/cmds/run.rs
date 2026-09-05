//! `air run` and `air serve`'s worker pool: linking a manifest and executing it.

use wasmtime::{Engine, Result};
use wasmtime_wasi::I32Exit;

use crate::link::link_all;
use crate::manifest::{Manifest, Target};

use crate::fail;

use super::build::build_if_needed;
use super::manifest_base;

/// What an invocation hands the guest: its arguments, and any directory the
/// user granted it. Both are host policy -- WASI defines `get-arguments` and
/// `get-directories`, but not what they answer -- so they arrive from the
/// command line rather than from the application.
#[derive(Default)]
pub struct GuestEnv {
    pub args: Vec<String>,
    /// Each granted directory and whether the guest may write in it.
    pub dirs: Vec<(String, bool)>,
    /// Whether the guest may open sockets. Off unless asked for, like a
    /// directory: `wasi:sockets` links either way, but every call answers
    /// `access-denied` until the host says otherwise.
    pub network: bool,
}

impl GuestEnv {
    pub fn with_args(mut self, args: &[String]) -> Self {
        self.args = args.to_vec();
        self
    }
}

pub fn run_manifest(engine: &Engine, path: &str, env: GuestEnv) -> Result<()> {
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
        return crate::component::run(engine, &manifest, &manifest_base(path), &env);
    }
    let base = manifest_base(path);
    let mut linked = link_all(engine, &manifest, &base)?;
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
