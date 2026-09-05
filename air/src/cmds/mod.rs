//! Subcommands: build / dist / run / serve / check / inspect / init / help,
//! plus the few helpers they share.

mod build;
mod check;
mod dist;
mod help;
mod inspect;
mod run;
mod scaffold;
mod serve;

pub use build::cmd_build;
pub use check::cmd_check;
pub use dist::cmd_dist;
pub use help::print_help;
pub use inspect::cmd_inspect;
pub use run::{GuestEnv, run_manifest};
pub use scaffold::{cmd_init, cmd_new};
pub use serve::cmd_serve;

use wasmtime::FuncType;

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

fn func_sig(t: &FuncType) -> String {
    let p: Vec<_> = t.params().map(|k| k.to_string()).collect();
    let r: Vec<_> = t.results().map(|k| k.to_string()).collect();
    format!("(func {} -> {})", p.join(" "), r.join(" "))
}
