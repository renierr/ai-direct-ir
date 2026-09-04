//! Subcommands: run / check / inspect / init / help.

use wasmtime::{Engine, ExternType, FuncType, Result, ValType};

use wasmtime_wasi::I32Exit;

use crate::link::link_all;
use crate::manifest::Manifest;

pub fn print_help() {
    println!(
        "host-rs {} — link + host configured WASM apps (see docs/19-harness.md)

USAGE:
  host-rs [command] [args]      run from a project directory or use paths

COMMANDS:
  (no arguments)                run ./host.toml
  run [manifest.toml]           link modules and execute; defaults to host.toml
  <manifest.toml>               shorthand for `run`
  check [manifest.toml]         link everything, verify wiring; defaults to host.toml
  inspect <module.wasm>         show imports/exports: what a (foreign) lib
                                needs and offers — start here for other
                                languages' .wasm output
  init <app.wasm>               scaffold a host.toml stub beside the app from its
                                  own imports (never overwrites)
  new <name>                    scaffold a project dir: <name>.wat +
                                  host.toml + README.md + AGENTS.md
                                 (never overwrites)
  help, -h, --help              this text
  version, -V, --version        version

EXAMPLES:
  host-rs run examples/server/manifest.toml
  host-rs check examples/server/manifest.toml
  cd myapp && host-rs
  host-rs inspect libs/sha256/sha256.wasm
  host-rs init myapp.wasm
  host-rs new myapp",
        env!("CARGO_PKG_VERSION")
    );
}

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

pub fn run_manifest(engine: &Engine, path: &str) -> Result<()> {
    let manifest: Manifest = crate::manifest::load(path)?;
    let base = manifest_base(path);
    if manifest.worker_count() > 1 {
        return run_workers(engine, path, &base);
    }
    let mut linked = link_all(engine, &manifest, &base)?;
    if linked.is_server {
        let run =
            linked
                .app_inst
                .get_typed_func::<i32, i32>(&mut linked.store, &linked.run_name)?;
        println!(
            "serving {:?} on 127.0.0.1:{} (Ctrl-C to stop)",
            manifest.root, linked.port
        );
        run.call(&mut linked.store, linked.port as i32)?;
        Ok(())
    } else {
        let func = linked
            .app_inst
            .get_func(&mut linked.store, &linked.run_name)
            .ok_or_else(|| {
                wasmtime::Error::msg(format!("app has no func {}", linked.run_name))
            })?;
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
}

fn func_sig(t: &FuncType) -> String {
    let p: Vec<_> = t.params().map(|k| k.to_string()).collect();
    let r: Vec<_> = t.results().map(|k| k.to_string()).collect();
    format!("(func {} -> {})", p.join(" "), r.join(" "))
}

/// Show what a (possibly foreign) module needs and offers — the starting
/// point for wiring any language's .wasm output into a manifest.
pub fn cmd_inspect(engine: &Engine, path: &str) -> Result<()> {
    let m = wasmtime::Module::from_file(engine, path)?;
    println!("module: {path}");
    println!("imports:");
    let mut needs_mem = false;
    let mut namespaces: Vec<String> = vec![];
    for imp in m.imports() {
        let ty = match imp.ty() {
            ExternType::Func(f) => func_sig(&f),
            ExternType::Memory(t) => {
                format!("(memory min={} max={:?})", t.minimum(), t.maximum())
            }
            ExternType::Global(_) => "(global)".into(),
            ExternType::Table(_) => "(table)".into(),
            _ => "(?)".into(),
        };
        println!("  {}.{} : {ty}", imp.module(), imp.name());
        if imp.module() == "env" && imp.name() == "memory" {
            needs_mem = true;
        } else if !namespaces.contains(&imp.module().to_string()) {
            namespaces.push(imp.module().to_string());
        }
    }
    println!("exports:");
    for exp in m.exports() {
        println!("  {}", exp.name());
    }
    println!("needs:");
    println!(
        "  shared env.memory: {}",
        if needs_mem { "yes" } else { "no (own memory)" }
    );
    println!("  namespaces: {}", namespaces.join(", "));
    Ok(())
}

/// Validate a manifest end-to-end without executing: links everything and
/// verifies the entry func signature.
pub fn cmd_check(engine: &Engine, manifest_path: &str, manifest: &Manifest) -> Result<()> {
    let base = manifest_base(manifest_path);
    let linked = link_all(engine, manifest, &base)?;
    let app_mod = wasmtime::Module::from_file(engine, crate::link::join(&base, &manifest.app.path))?;
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

fn fail<T>(msg: String) -> Result<T> {
    Err(wasmtime::Error::msg(msg))
}

/// Server with host-owned accept loop: the main thread accepts, N worker
/// threads each own a fully linked instance and run `handle(cfd)` per
/// connection. Blocking sockets + OS threads, std only: no async runtime,
/// no new deps. One worker dying (trap) costs its connection, not the server.
fn run_workers(engine: &Engine, path: &str, base: &std::path::Path) -> Result<()> {
    use crate::host::Sock;
    use std::sync::mpsc;

    let manifest: Manifest = crate::manifest::load(path)?;
    let port = manifest.port.unwrap_or(8123);
    let n = manifest.worker_count();
    let entry = manifest.app.run.clone();
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| wasmtime::Error::msg(format!("bind 127.0.0.1:{port}: {e}")))?;
    println!(
        "serving {:?} on 127.0.0.1:{port} with {n} workers (Ctrl-C to stop)",
        manifest.root
    );
    let mut txs = Vec::new();
    for w in 0..n {
        let (tx, rx) = mpsc::channel::<std::net::TcpStream>();
        txs.push(tx);
        let eng = engine.clone();
        let mpath = path.to_string();
        let bdir = base.to_path_buf();
        let func = entry.clone();
        std::thread::Builder::new()
            .name(format!("worker-{w}"))
            .spawn(move || {
                let m: Manifest = match crate::manifest::load(&mpath) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("worker-{w}: manifest: {e}");
                        return;
                    }
                };
                let mut linked = match link_all(&eng, &m, &bdir) {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("worker-{w}: link: {e}");
                        return;
                    }
                };
                let handle =
                    match linked
                        .app_inst
                        .get_typed_func::<i32, i32>(&mut linked.store, &func)
                    {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("worker-{w}: entry `{func}`: {e}");
                            return;
                        }
                    };
                for stream in rx {
                    let h = linked.store.data_mut().alloc_sock(Sock::Conn(stream));
                    if let Err(e) = handle.call(&mut linked.store, h) {
                        eprintln!("worker-{w}: handle: {e}");
                    }
                    // Host closes: the handle is dropped here either way,
                    // so a leaky app can't exhaust the socket table.
                    linked.store.data_mut().socks.remove(&h);
                }
            })
            .map_err(|e| wasmtime::Error::msg(format!("spawn worker: {e}")))?;
    }
    let mut i = 0usize;
    for conn in listener.incoming() {
        match conn {
            Ok(c) => {
                let _ = txs[i % n].send(c);
                i += 1;
            }
            Err(e) => eprintln!("accept: {e}"),
        }
    }
    Ok(())
}

/// Scaffold a full project dir: starter .wat + host.toml + README + AGENTS.
/// Templates are baked into the binary (include_str!), so a fresh project
/// carries harness instructions and rules with it. Never overwrites.
pub fn cmd_new(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return fail(format!("bad project name `{name}`: use [A-Za-z0-9_-]"));
    }
    let dir = std::path::Path::new(name);
    if dir.exists() {
        let empty = std::fs::read_dir(dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !empty {
            return fail(format!("`{name}` exists and is not empty, refusing"));
        }
    } else {
        std::fs::create_dir_all(dir)?;
    }
    let wat = dir.join(format!("{name}.wat"));
    let toml = dir.join("host.toml");
    let readme = dir.join("README.md");
    let agents = dir.join("AGENTS.md");
    for p in [&wat, &toml, &readme, &agents] {
        if p.exists() {
            return fail(format!("`{}` exists, refusing to overwrite", p.display()));
        }
    }
    let hello = format!("hello from {name}\n");
    let starter = format!(
        ";; {name}.wat — {name} app, hosted by host-rs.\n\
         ;; Build: wat2wasm {name}.wat -o {name}.wasm\n\
         ;; Check: host-rs check\n\
         ;; Run:   host-rs\n\
         ;;\n\
         ;; Command-mode contract: own memory (export it for WASI),\n\
         ;; WASI stdio, `_start` entry, `proc_exit` code is the exit code.\n\
         ;; Need sockets, shared libs, or bridges? New needs go in the\n\
         ;; manifest (TOML), never in harness code.\n\
         \n\
         (module\n\
         \x20 (import \"wasi_snapshot_preview1\" \"fd_write\"\n\
         \x20   (func $fd_write (param i32 i32 i32 i32) (result i32)))\n\
         \x20 (import \"wasi_snapshot_preview1\" \"proc_exit\"\n\
         \x20   (func $exit (param i32)))\n\
         \x20 (memory 1)\n\
         \x20 (export \"memory\" (memory 0))\n\
         \n\
         \x20 (func (export \"_start\")\n\
         \x20   (i32.store (i32.const 0) (i32.const 0x1000))\n\
         \x20   (i32.store (i32.const 4) (i32.const {hlen}))\n\
         \x20   (call $fd_write (i32.const 1) (i32.const 0)\n\
         \x20     (i32.const 1) (i32.const 8))\n\
         \x20   (drop)\n\
         \x20   (call $exit (i32.const 0))\n\
         \x20   (unreachable))\n\
         \n\
         \x20 (data (i32.const 0x1000) \"{hello}\")\n\
         )\n",
        name = name,
        // WAT string syntax needs an escaped newline, not a literal one;
        // otherwise the byte count includes a byte absent from the data.
        hello = hello.escape_default(),
        hlen = hello.len()
    );
    let manifest = format!(
        "# {name}: command-mode app. Build the .wasm first:\n\
         #   wat2wasm {name}.wat -o {name}.wasm\n\
         # then: host-rs check && host-rs\n\
         mode = \"command\"\n\
         \n\
         [app]\n\
         path = \"{name}.wasm\"\n\
         run = \"_start\"\n"
    );
    std::fs::write(&wat, starter)?;
    std::fs::write(&toml, manifest)?;
    std::fs::write(
        &readme,
        include_str!("../templates/project-readme.md").replace("__APPNAME__", name),
    )?;
    std::fs::write(
        &agents,
        include_str!("../templates/project-agents.md").replace("__APPNAME__", name),
    )?;
    println!(
        "created {name}/:\n  {name}.wat\n  host.toml\n  README.md\n  AGENTS.md\n\
         next:\n  cd {name} && wat2wasm {name}.wat -o {name}.wasm && host-rs check"
    );
    Ok(())
}

/// Scaffold a manifest stub from an app module's own imports.
pub fn cmd_init(engine: &Engine, app_path: &str) -> Result<()> {
    let m = wasmtime::Module::from_file(engine, app_path)?;
    let mut namespaces: Vec<String> = vec![];
    let mut wants_net = false;
    for imp in m.imports() {
        match imp.module() {
            "env" | "wasi_snapshot_preview1" => {}
            "net" => wants_net = true,
            ns => {
                if !namespaces.contains(&ns.to_string()) {
                    namespaces.push(ns.to_string());
                }
            }
        }
    }
    let stem = std::path::Path::new(app_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "app".into());
    let dir = std::path::Path::new(app_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pref = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut out = String::new();
    if wants_net {
        out.push_str("mode = \"server\"\nport = 8123\n");
        out.push_str("# root = \"www\"  # uncomment to preopen a data dir (WASI fd 3)\n");
    } else {
        out.push_str("mode = \"command\"\n");
    }
    out.push_str("# memory_pages = 2  # floor; import minima always win\n\n");
    for ns in &namespaces {
        out.push_str(&format!(
            "[[libs]]  # `{ns}` wanted by {stem}.wasm — point at the providing module\npath = \"<lib-{ns}.wasm>\"\nas = \"{ns}\"\n\n"
        ));
    }
    if namespaces.is_empty() {
        out.push_str("# no custom namespaces: app needs only env/net/wasi\n\n");
    }
    let run = if wants_net { "run" } else { "_start" };
    out.push_str(&format!(
        "[app]\npath = \"{pref}{stem}.wasm\"\nrun = \"{run}\"\n"
    ));
    let toml_path = format!("{pref}host.toml");
    // Never silently overwrite an existing manifest.
    if std::path::Path::new(&toml_path).exists() {
        return Err(wasmtime::Error::msg(format!(
            "{toml_path} exists, refusing to overwrite"
        )));
    }
    std::fs::write(&toml_path, &out)?;
    println!("wrote {toml_path}:\n{out}");
    Ok(())
}
