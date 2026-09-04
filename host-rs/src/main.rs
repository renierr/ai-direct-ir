//! Generic harness for AI-direct-IR apps: built once, driven by a manifest.
//!
//! One native binary runs every project in this repo — servers, CLI tools,
//! present and future. A new app means a new TOML manifest + `.wasm` files,
//! never a harness rebuild. Usage (from the repo root):
//!
//!   host-rs srv/manifest.toml     # server mode: links libs, serves
//!   host-rs src/pi.toml           # command mode: runs _start with stdio
//!   echo 100 | host-rs src/pi.toml
//!
//! The harness does four generic jobs (see docs/19-harness.md):
//!  1. Own the ONE shared memory (`env`), sized from manifest + import minima.
//!  2. Provide `net.*` TCP syscalls over std::net (WASI preview1 has no listen).
//!  3. Link `[[libs]]` (shared memory, zero-copy) and `[[bridges]]`
//!     (own memory, host copies buffers across) into the app.
//!  4. Run: `run(port)` in server mode, `run()` in command mode.
//!
//! v1 limits (extend once, all apps benefit): bridge calls are fixed-arity
//! `(in_ptr, in_len, out_ptr) -> rc`; syscalls are TCP-only.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use serde::Deserialize;
use wasmtime::{
    Caller, Engine, ExternType, Linker, Memory, MemoryType, Module, Result, Store,
};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::{FsPerms, I32Exit, WasiCtxBuilder};

// ---------------- manifest ----------------

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Server,
    Command,
}

#[derive(Deserialize)]
struct Lib {
    path: String,
    #[serde(rename = "as")]
    namespace: String,
}

#[derive(Deserialize, Clone)]
struct BridgeCall {
    #[serde(rename = "as")]
    name: String,
    func: String,
    in_ptr: usize,
    in_len: usize,
    out_ptr: usize,
    out_len: u32,
    #[serde(default = "default_max_in")]
    max_in: u32,
}

fn default_max_in() -> u32 {
    1 << 20
}

#[derive(Deserialize)]
struct Bridge {
    path: String,
    #[serde(rename = "as")]
    namespace: String,
    alloc: String,
    calls: Vec<BridgeCall>,
}

#[derive(Deserialize)]
struct App {
    path: String,
    run: String,
}

#[derive(Deserialize)]
struct Manifest {
    mode: Mode,
    port: Option<u16>,
    root: Option<String>,
    guest: Option<String>,
    memory_pages: Option<u32>,
    #[serde(default)]
    libs: Vec<Lib>,
    #[serde(default)]
    bridges: Vec<Bridge>,
    app: App,
}

// ---------------- host state ----------------

enum Sock {
    Listen(TcpListener),
    Conn(TcpStream),
}

struct Host {
    wasi: WasiP1Ctx,
    socks: HashMap<i32, Sock>,
    next: i32,
    /// The ONE shared memory. Guests may re-export it; the host keeps its
    /// own handle and never depends on guest exports to find it.
    shared: Option<Memory>,
}

impl Host {
    fn alloc_sock(&mut self, s: Sock) -> i32 {
        let h = self.next;
        self.next += 1;
        self.socks.insert(h, s);
        h
    }
}

fn shared_mem(caller: &mut Caller<'_, Host>) -> Result<Memory> {
    caller
        .data()
        .shared
        .clone()
        .ok_or_else(|| wasmtime::Error::msg("harness memory not installed"))
}

// ---------------- net.* syscalls ----------------

fn w_listen(mut caller: Caller<'_, Host>, port: i32) -> Result<i32> {
    let l = match TcpListener::bind(("127.0.0.1", port as u16)) {
        Ok(l) => l,
        Err(_) => return Ok(-1),
    };
    Ok(caller.data_mut().alloc_sock(Sock::Listen(l)))
}

fn w_accept(mut caller: Caller<'_, Host>, h: i32) -> Result<i32> {
    let conn = match caller.data().socks.get(&h) {
        Some(Sock::Listen(l)) => match l.accept() {
            Ok((c, _)) => c,
            Err(_) => return Ok(-1),
        },
        _ => return Ok(-1),
    };
    Ok(caller.data_mut().alloc_sock(Sock::Conn(conn)))
}

fn w_recv(mut caller: Caller<'_, Host>, h: i32, ptr: i32, len: i32) -> Result<i32> {
    let mut buf = vec![0u8; (len as usize).min(65536)];
    let n = match caller.data_mut().socks.get_mut(&h) {
        Some(Sock::Conn(c)) => match c.read(&mut buf) {
            Ok(n) => n,
            Err(_) => return Ok(-1),
        },
        _ => return Ok(-1),
    };
    if n == 0 {
        return Ok(0);
    }
    shared_mem(&mut caller)?.write(&mut caller, ptr as usize, &buf[..n])?;
    Ok(n as i32)
}

fn w_send(mut caller: Caller<'_, Host>, h: i32, ptr: i32, len: i32) -> Result<i32> {
    let mut buf = vec![0u8; len as usize];
    shared_mem(&mut caller)?.read(&caller, ptr as usize, &mut buf)?;
    match caller.data_mut().socks.get_mut(&h) {
        Some(Sock::Conn(c)) => match c.write(&buf) {
            Ok(n) => Ok(n as i32),
            Err(_) => Ok(-1),
        },
        _ => Ok(-1),
    }
}

fn w_close(mut caller: Caller<'_, Host>, h: i32) -> Result<i32> {
    caller.data_mut().socks.remove(&h);
    Ok(0)
}

// ---------------- main ----------------

fn needs_shared_mem(engine: &Engine, paths: &[&str]) -> Result<(bool, u32)> {
    let mut needed = false;
    let mut min_pages = 0u32;
    for p in paths {
        let m = Module::from_file(engine, p)?;
        for imp in m.imports() {
            if imp.module() == "env" && imp.name() == "memory" {
                needed = true;
                if let ExternType::Memory(t) = imp.ty() {
                    min_pages = min_pages.max(t.minimum().try_into().unwrap_or(u32::MAX));
                }
            }
        }
    }
    Ok((needed, min_pages))
}

fn wire_lib(
    linker: &mut Linker<Host>,
    store: &mut Store<Host>,
    engine: &Engine,
    lib: &Lib,
) -> Result<()> {
    let module = Module::from_file(engine, &lib.path)?;
    // Auto-wire EVERY export under the manifest namespace: the export list
    // IS the contract, no hardcoded names.
    let names: Vec<String> = module.exports().map(|e| e.name().to_string()).collect();
    let inst = linker.instantiate(&mut *store, &module)?;
    for name in names {
        let ext = inst.get_export(&mut *store, &name).ok_or_else(|| {
            wasmtime::Error::msg(format!("{} lost export {name}", lib.path))
        })?;
        linker.define(&mut *store, &lib.namespace, &name, ext)?;
    }
    Ok(())
}

fn wire_bridge(
    linker: &mut Linker<Host>,
    store: &mut Store<Host>,
    engine: &Engine,
    app_shared: Option<Memory>,
    bridge: &Bridge,
) -> Result<()> {
    let module = Module::from_file(engine, &bridge.path)?;
    let wants_shared = module
        .imports()
        .any(|i| i.module() == "env" && i.name() == "memory");
    let inst = linker.instantiate(&mut *store, &module)?;
    // Shared-style lib uses the app memory; own-memory lib gets bridged.
    let lib_mem = if wants_shared {
        app_shared.ok_or_else(|| {
            wasmtime::Error::msg(format!("{} wants env.memory, none created", bridge.path))
        })?
    } else {
        inst.get_memory(&mut *store, "memory").ok_or_else(|| {
            wasmtime::Error::msg(format!("{} has no memory export", bridge.path))
        })?
    };
    let alloc = inst.get_typed_func::<i32, i32>(&mut *store, &bridge.alloc)?;
    for call in &bridge.calls {
        let desc = call.clone();
        let func = inst.get_typed_func::<(i32, i32, i32), i32>(&mut *store, &call.func)?;
        let (cmem, cal, cfunc) = (lib_mem, alloc.clone(), func.clone());
        let (ns, nm) = (bridge.namespace.clone(), call.name.clone());
        linker.func_wrap(
            &ns,
            &nm,
            move |mut caller: Caller<'_, Host>, a: i32, b: i32, c: i32| -> Result<i32> {
                let args = [a, b, c];
                let (in_ptr, in_len, out_ptr) =
                    (args[desc.in_ptr], args[desc.in_len], args[desc.out_ptr]);
                if in_len < 0 || in_len as u32 > desc.max_in {
                    return Ok(-1);
                }
                let app_mem = shared_mem(&mut caller)?;
                let mut input = vec![0u8; in_len as usize];
                app_mem.read(&caller, in_ptr as usize, &mut input)?;
                let wise_in = cal.call(&mut caller, in_len)?;
                cmem.write(&mut caller, wise_in as usize, &input)?;
                let wise_out = cal.call(&mut caller, desc.out_len as i32)?;
                let rc = cfunc.call(&mut caller, (wise_in, in_len, wise_out))?;
                if rc != 0 {
                    return Ok(-1);
                }
                let mut out = vec![0u8; desc.out_len as usize];
                cmem.read(&caller, wise_out as usize, &mut out)?;
                app_mem.write(&mut caller, out_ptr as usize, &out)?;
                Ok(0)
            },
        )?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let manifest_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "srv/manifest.toml".into());
    let manifest: Manifest = toml::from_str(&std::fs::read_to_string(&manifest_path)?)?;

    let engine = Engine::default();

    // WASI: stdio always inherited (commands need it, servers ignore it);
    // optional single preopen (first non-stdio fd, classically 3).
    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();
    if let Some(root) = &manifest.root {
        let guest = manifest.guest.clone().unwrap_or_else(|| {
            std::path::Path::new(root)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "root".into())
        });
        builder.preopened_dir(root, guest, FsPerms::ReadOnly)?;
    }
    let wasi = builder.build_p1();

    let mut store = Store::new(
        &engine,
        Host {
            wasi,
            socks: HashMap::new(),
            next: 100,
            shared: None,
        },
    );
    let mut linker = Linker::<Host>::new(&engine);
    p1::add_to_linker_sync(&mut linker, |h| &mut h.wasi)?;

    // Shared memory iff some module imports env.memory, sized to cover
    // both the manifest request and every importer's minimum.
    let mut all_paths: Vec<&str> = manifest
        .libs
        .iter()
        .map(|l| l.path.as_str())
        .chain(manifest.bridges.iter().map(|b| b.path.as_str()))
        .chain(std::iter::once(manifest.app.path.as_str()))
        .collect();
    all_paths.sort();
    all_paths.dedup();
    let (want_shared, import_min) = needs_shared_mem(&engine, &all_paths)?;
    let app_shared = if want_shared {
        let pages = manifest.memory_pages.unwrap_or(1).max(import_min).max(1);
        let mem = Memory::new(&mut store, MemoryType::new(pages, Some(pages)))?;
        store.data_mut().shared = Some(mem);
        linker.define(&mut store, "env", "memory", mem)?;
        Some(mem)
    } else {
        None
    };

    linker.func_wrap("net", "listen", w_listen)?;
    linker.func_wrap("net", "accept", w_accept)?;
    linker.func_wrap("net", "recv", w_recv)?;
    linker.func_wrap("net", "send", w_send)?;
    linker.func_wrap("net", "close", w_close)?;

    // Libs first (only env + syscalls), then bridges, then the app.
    for lib in &manifest.libs {
        wire_lib(&mut linker, &mut store, &engine, lib)?;
    }
    for bridge in &manifest.bridges {
        wire_bridge(&mut linker, &mut store, &engine, app_shared, bridge)?;
    }

    let app_mod = Module::from_file(&engine, &manifest.app.path)?;
    let app_inst = linker.instantiate(&mut store, &app_mod)?;
    match manifest.mode {
        Mode::Server => {
            let port = manifest.port.unwrap_or(8123);
            let run = app_inst.get_typed_func::<i32, i32>(&mut store, &manifest.app.run)?;
            println!("serving {:?} on 127.0.0.1:{port} (Ctrl-C to stop)", manifest.root);
            run.call(&mut store, port as i32)?;
            Ok(())
        }
        Mode::Command => {
            let func = app_inst
                .get_func(&mut store, &manifest.app.run)
                .ok_or_else(|| {
                    wasmtime::Error::msg(format!("app has no func {}", manifest.app.run))
                })?;
            match func.call(&mut store, &[], &mut []) {
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
}
