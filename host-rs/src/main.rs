//! Scaffolding host for the AI-direct-IR static file server, in Rust.
//!
//! Same three jobs as the retired `srv/serve.py`: own the ONE shared memory
//! (imported by both modules as `env`), implement the five `net.*` TCP
//! syscalls over real OS sockets, and link `lib/http.wasm` into
//! `srv/server.wasm`. All HTTP parsing, routing and file serving still run
//! 100% inside the two .wasm modules.
//!
//! Run from the repo root: `cargo run -p host-rs -- 8123`
//! (needs the `wasmtime`/`wasmtime-wasi` crates at BUILD time only;
//! the shipped server is this one native binary + the two .wasm files).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use wasmtime::{
    Caller, Engine, Linker, Memory, MemoryType, Module, Result, Store, TypedFunc,
};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

enum Sock {
    Listen(TcpListener),
    Conn(TcpStream),
}

struct Host {
    wasi: WasiP1Ctx,
    socks: HashMap<i32, Sock>,
    next: i32,
    // The ONE shared memory, owned by the harness. Guests may re-export it,
    // but the host never depends on their exports to find it.
    shared: Option<Memory>,
    // Finished-lib bridge: the Rust sha256 module keeps its OWN memory
    // (std assumes ownership), so the harness copies buffers across.
    sha_mem: Option<Memory>,
    sha_alloc: Option<TypedFunc<i32, i32>>,
    sha_hash: Option<TypedFunc<(i32, i32, i32), i32>>,
}

impl Host {
    fn alloc(&mut self, s: Sock) -> i32 {
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

fn w_listen(mut caller: Caller<'_, Host>, port: i32) -> Result<i32> {
    let l = match TcpListener::bind(("127.0.0.1", port as u16)) {
        Ok(l) => l,
        Err(_) => return Ok(-1),
    };
    Ok(caller.data_mut().alloc(Sock::Listen(l)))
}

fn w_accept(mut caller: Caller<'_, Host>, h: i32) -> Result<i32> {
    let conn = match caller.data().socks.get(&h) {
        Some(Sock::Listen(l)) => match l.accept() {
            Ok((c, _)) => c,
            Err(_) => return Ok(-1),
        },
        _ => return Ok(-1),
    };
    Ok(caller.data_mut().alloc(Sock::Conn(conn)))
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

/// bridge.sha256(app_ptr, app_len, app_out64): hash app-memory bytes with the
/// Rust lib, write 64 hex bytes back. Returns 0 ok, -1 fail.
fn w_bridge(
    mut caller: Caller<'_, Host>,
    app_ptr: i32,
    app_len: i32,
    app_out: i32,
) -> Result<i32> {
    let app_mem = shared_mem(&mut caller)?;
    let (sha_mem, sha_alloc, sha_hash) = match (
        caller.data().sha_mem.clone(),
        caller.data().sha_alloc.clone(),
        caller.data().sha_hash.clone(),
    ) {
        (Some(m), Some(a), Some(h)) => (m, a, h),
        _ => return Ok(-1),
    };
    if app_len < 0 || app_len > 7000 {
        return Ok(-1);
    }
    let mut input = vec![0u8; app_len as usize];
    app_mem.read(&caller, app_ptr as usize, &mut input)?;
    let in_ptr = sha_alloc.call(&mut caller, app_len)?;
    sha_mem.write(&mut caller, in_ptr as usize, &input)?;
    let out_ptr = sha_alloc.call(&mut caller, 64)?;
    let rc = sha_hash.call(&mut caller, (in_ptr, app_len, out_ptr))?;
    if rc != 0 {
        return Ok(-1);
    }
    let mut hex = vec![0u8; 64];
    sha_mem.read(&caller, out_ptr as usize, &mut hex)?;
    app_mem.write(&mut caller, app_out as usize, &hex)?;
    Ok(0)
}

fn main() -> Result<()> {
    let port: u16 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "8123".into())
        .parse()?;

    let engine = Engine::default();
    let mut builder = WasiCtxBuilder::new();
    // Single preopen -> WASI fd 3, exactly like the Python host did.
    builder.preopened_dir("srv/www", "www", FsPerms::ReadOnly)?;
    let wasi = builder.build_p1();

    let mut store = Store::new(
        &engine,
        Host {
            wasi,
            socks: HashMap::new(),
            next: 100,
            shared: None,
            sha_mem: None,
            sha_alloc: None,
            sha_hash: None,
        },
    );
    let mut linker = Linker::<Host>::new(&engine);
    p1::add_to_linker_sync(&mut linker, |h| &mut h.wasi)?;

    let mem = Memory::new(&mut store, MemoryType::new(2, Some(2)))?;
    store.data_mut().shared = Some(mem);
    linker.define(&mut store, "env", "memory", mem)?;

    linker.func_wrap("net", "listen", w_listen)?;
    linker.func_wrap("net", "accept", w_accept)?;
    linker.func_wrap("net", "recv", w_recv)?;
    linker.func_wrap("net", "send", w_send)?;
    linker.func_wrap("net", "close", w_close)?;

    // Finished Rust lib: own memory, bridged by copying (see w_bridge).
    let sha_mod = Module::from_file(&engine, "lib/sha256.wasm")?;
    let sha_inst = linker.instantiate(&mut store, &sha_mod)?;
    let sha_mem = sha_inst
        .get_memory(&mut store, "memory")
        .ok_or_else(|| wasmtime::Error::msg("sha lib has no memory"))?;
    let sha_alloc = sha_inst.get_typed_func::<i32, i32>(&mut store, "sha256_alloc")?;
    let sha_hash =
        sha_inst.get_typed_func::<(i32, i32, i32), i32>(&mut store, "sha256_hex")?;
    store.data_mut().sha_mem = Some(sha_mem);
    store.data_mut().sha_alloc = Some(sha_alloc);
    store.data_mut().sha_hash = Some(sha_hash);
    linker.func_wrap("bridge", "sha256", w_bridge)?;

    // Lib first (needs only env + net.send), then app (needs lib.* too).
    // Exports are wired explicitly: the lib's export list IS the contract.
    let lib_mod = Module::from_file(&engine, "lib/http.wasm")?;
    let lib_inst = linker.instantiate(&mut store, &lib_mod)?;
    for name in [
        "send_all",
        "send_status",
        "send_header",
        "send_crlf",
        "send_clen",
        "mime_for",
        "parse_request",
    ] {
        let ext = lib_inst
            .get_export(&mut store, name)
            .ok_or_else(|| wasmtime::Error::msg(format!("lib missing export {name}")))?;
        linker.define(&mut store, "lib", name, ext)?;
    }

    let app_mod = Module::from_file(&engine, "srv/server.wasm")?;
    let app_inst = linker.instantiate(&mut store, &app_mod)?;
    let run = app_inst.get_typed_func::<i32, i32>(&mut store, "run")?;

    println!("serving srv/www/ on 127.0.0.1:{port} (Ctrl-C to stop)");
    run.call(&mut store, port as i32)?;
    Ok(())
}
