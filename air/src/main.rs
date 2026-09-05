//! air: link + host configured WASM apps. See docs/PROJECT.md.

mod asm;
mod boundary;
mod cmds;
mod component;
mod gui;
mod host;
mod link;
mod manifest;
mod net;
mod term;
mod wit;

use wasmtime::{Engine, Result};

/// The crate's shorthand for "stop with this message". Every subcommand and
/// every assembler stage reports failure the same way.
fn fail<T>(msg: String) -> Result<T> {
    Err(wasmtime::Error::msg(msg))
}

/// Read leading host options off `run`. Everything from the first
/// non-option word on is the manifest and then the guest's own arguments, so
/// an application never has to escape its flags away from `air`'s.
fn guest_env(args: &[String]) -> wasmtime::Result<(cmds::GuestEnv, &[String])> {
    let mut env = cmds::GuestEnv::default();
    let mut rest = args;
    while let Some(first) = rest.first() {
        match first.as_str() {
            // Writing is opt-in at the grant, so a tool cannot modify a
            // directory that was only meant to be read.
            "--dir" | "--dir-rw" => {
                let Some(dir) = rest.get(1) else {
                    return Err(wasmtime::Error::msg(format!(
                        "`{first}` needs a directory: air run {first} <path> <manifest> [args...]"
                    )));
                };
                env.dirs.push((dir.clone(), first == "--dir-rw"));
                rest = &rest[2..];
            }
            _ => break,
        }
    }
    Ok((env, rest))
}

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
            let (env, rest) = guest_env(&args[1..])?;
            let (path, rest) = match rest.split_first() {
                Some((first, tail)) => (first.as_str(), tail),
                None => ("host.toml", rest),
            };
            cmds::run_manifest(&engine, path, env.with_args(rest))
        }
        other => {
            // Shorthand: a .toml path means `run`.
            if other.ends_with(".toml") {
                cmds::run_manifest(
                    &engine,
                    other,
                    cmds::GuestEnv::default().with_args(&args[1..]),
                )
            } else {
                cmds::print_help();
                Err(wasmtime::Error::msg(format!("unknown command `{other}`")))
            }
        }
    }
}
