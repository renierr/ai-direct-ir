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
    if manifest.worker_count() > 1 {
        return run_workers(engine, path, &base);
    }
    let mut linked = link_all(engine, &manifest, &base)?;
    if linked.is_server {
        let run = linked
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
}

/// Server with host-owned accept loop: the main thread accepts, N worker
/// threads each own a fully linked instance and run `handle(cfd)` per
/// connection. Blocking sockets + OS threads, std only: no async runtime,
/// no new deps. One worker dying (trap) costs its connection, not the server.
pub(super) fn run_workers(engine: &Engine, path: &str, base: &std::path::Path) -> Result<()> {
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
                let handle = match linked
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
