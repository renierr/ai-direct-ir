//! `air run`: linking a manifest and executing it.

use wasmtime::{Engine, Result};

use crate::manifest::{Manifest, Mode, Target};

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
    let base = manifest_base(path);
    // `gui` differs from `command` only in who calls the entry point and how
    // often: the linking, the grants and the WASI boundary are the same.
    match manifest.mode {
        Mode::Gui => crate::gui::run(engine, &manifest, &base),
        Mode::Command => crate::component::run(engine, &manifest, &base, &env),
    }
}
