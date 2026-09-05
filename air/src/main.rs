//! air: link + host configured WASM apps. See docs/PROJECT.md.

mod boundary;
mod cmds;
mod component;
mod gui;
mod host;
mod link;
mod manifest;
mod net;
mod term;

use wasmtime::{Engine, Result};

fn usage_err(what: &str, usage: &str) -> Result<()> {
    Err(wasmtime::Error::msg(format!("{what}\nusage: {usage}")))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let engine = Engine::default();
    // Commands are explicit; no arguments always show the available workflow.
    if args.is_empty() {
        cmds::print_help();
        return Ok(());
    }
    let arg1 = args.get(1).map(|s| s.as_str()).unwrap_or("");
    match args[0].as_str() {
        "help" | "-h" | "--help" => {
            cmds::print_help();
            Ok(())
        }
        "version" | "-V" | "--version" => {
            println!("air {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "inspect" => {
            if arg1.is_empty() {
                return usage_err("missing module", "air inspect <module.wasm>");
            }
            cmds::cmd_inspect(&engine, arg1)
        }
        "check" => {
            let path = if arg1.is_empty() { "host.toml" } else { arg1 };
            let manifest = manifest::load(path)?;
            cmds::cmd_check(&engine, path, &manifest)
        }
        "build" => {
            let path = if arg1.is_empty() { "host.toml" } else { arg1 };
            cmds::cmd_build(&engine, path)
        }
        "dist" => {
            let path = if arg1.is_empty() { "host.toml" } else { arg1 };
            cmds::cmd_dist(path)
        }
        "serve" => {
            let path = if arg1.is_empty() { "host.toml" } else { arg1 };
            cmds::cmd_serve(path)
        }
        "init" => {
            if arg1.is_empty() {
                return usage_err("missing app module", "air init <app.wasm>");
            }
            cmds::cmd_init(&engine, arg1)
        }
        "new" => {
            if arg1.is_empty() {
                return usage_err("missing project name", "air new <name>");
            }
            cmds::cmd_new(&engine, arg1)
        }
        // Everything after the manifest belongs to the guest, not to `air`.
        "run" => {
            let (path, rest) = if arg1.is_empty() {
                ("host.toml", &args[1..])
            } else {
                (arg1, &args[2..])
            };
            cmds::run_manifest(&engine, path, rest)
        }
        other => {
            // Shorthand: a .toml path means `run`.
            if other.ends_with(".toml") {
                cmds::run_manifest(&engine, other, &args[1..])
            } else {
                cmds::print_help();
                Err(wasmtime::Error::msg(format!("unknown command `{other}`")))
            }
        }
    }
}
