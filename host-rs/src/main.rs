//! host-rs: link + host configured WASM apps. See docs/19-harness.md.

mod cmds;
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
            println!("host-rs {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "inspect" => {
            if arg1.is_empty() {
                return usage_err("missing module", "host-rs inspect <module.wasm>");
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
            cmds::cmd_build(path)
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
                return usage_err("missing app module", "host-rs init <app.wasm>");
            }
            cmds::cmd_init(&engine, arg1)
        }
        "new" => {
            if arg1.is_empty() {
                return usage_err("missing project name", "host-rs new <name>");
            }
            cmds::cmd_new(arg1)
        }
        "run" => cmds::run_manifest(&engine, if arg1.is_empty() { "host.toml" } else { arg1 }),
        other => {
            // Shorthand: a .toml path means `run`.
            if other.ends_with(".toml") {
                cmds::run_manifest(&engine, other)
            } else {
                cmds::print_help();
                Err(wasmtime::Error::msg(format!("unknown command `{other}`")))
            }
        }
    }
}
