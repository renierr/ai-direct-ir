//! Linking: shared memory, libs (zero-copy), bridges (copy), app.
//!
//! Order matters: libs first (env + syscalls only), then bridges, then the
//! app. Instantiation itself proves every import is satisfied.

use std::path::{Path, PathBuf};

use wasmtime::{Caller, Engine, ExternType, Linker, Memory, MemoryType, Module, Result, Store};

use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::{FsPerms, WasiCtxBuilder};

use crate::gui;
use crate::host::{Host, shared_mem};
use crate::manifest::{Bridge, Lib, Manifest};
use crate::term;

/// Resolve a manifest path: manifest dir wins, CWD is the fallback, so both
/// `air srv/manifest.toml` from the root and from anywhere else work.
pub fn join(base: &Path, p: &str) -> PathBuf {
    let q = Path::new(p);
    if q.is_absolute() {
        return q.to_path_buf();
    }
    let via_manifest = base.join(q);
    if via_manifest.exists() {
        via_manifest
    } else {
        q.to_path_buf()
    }
}

fn needs_shared_mem(engine: &Engine, paths: &[PathBuf]) -> Result<(bool, u32)> {
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
    module_path: &Path,
    lib: &Lib,
) -> Result<()> {
    let module = Module::from_file(engine, module_path)?;
    // Auto-wire EVERY export under the manifest namespace: the export list
    // IS the contract, no hardcoded names.
    let names: Vec<String> = module.exports().map(|e| e.name().to_string()).collect();
    let inst = linker.instantiate(&mut *store, &module)?;
    for name in names {
        let ext = inst
            .get_export(&mut *store, &name)
            .ok_or_else(|| wasmtime::Error::msg(format!("{} lost export {name}", lib.path)))?;
        linker.define(&mut *store, &lib.namespace, &name, ext)?;
    }
    Ok(())
}

fn wire_bridge(
    linker: &mut Linker<Host>,
    store: &mut Store<Host>,
    engine: &Engine,
    module_path: &Path,
    app_shared: Option<Memory>,
    bridge: &Bridge,
) -> Result<()> {
    let module = Module::from_file(engine, module_path)?;
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
        inst.get_memory(&mut *store, "memory")
            .ok_or_else(|| wasmtime::Error::msg(format!("{} has no memory export", bridge.path)))?
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

pub struct Linked {
    pub store: Store<Host>,
    pub app_inst: wasmtime::Instance,
    pub run_name: String,
}

/// Load + link everything, stop before executing. Shared by `run` and `check`.
/// `base` is the manifest's directory: relative paths resolve against it,
// (not against the process CWD), so manifests are relocatable.
pub fn link_all(engine: &Engine, manifest: &Manifest, base: &Path) -> Result<Linked> {
    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();
    if let Some(root) = &manifest.root {
        let guest = manifest.guest.clone().unwrap_or_else(|| {
            std::path::Path::new(root)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "root".into())
        });
        builder.preopened_dir(join(base, root), guest, FsPerms::ReadOnly)?;
    }
    let wasi: WasiP1Ctx = builder.build_p1();

    let mut store = Store::new(&engine, Host::new(wasi));
    let mut linker = Linker::<Host>::new(engine);
    p1::add_to_linker_sync(&mut linker, |h| &mut h.wasi)?;

    let mut all_paths: Vec<PathBuf> = manifest
        .libs
        .iter()
        .map(|l| join(base, &l.path))
        .chain(manifest.bridges.iter().map(|b| join(base, &b.path)))
        .chain(std::iter::once(join(base, &manifest.app.path)))
        .collect();
    all_paths.sort();
    all_paths.dedup();
    let (want_shared, import_min) = needs_shared_mem(engine, &all_paths)?;
    if want_shared {
        // Manifest floor + every importer's declared minimum win.
        let pages = manifest.memory_pages.unwrap_or(1).max(import_min).max(1);
        let mem = Memory::new(&mut store, MemoryType::new(pages, Some(pages)))?;
        store.data_mut().shared = Some(mem);
        linker.define(&mut store, "env", "memory", mem)?;
    }

    linker.func_wrap("term", "enter", term::w_enter)?;
    linker.func_wrap("term", "available", term::w_available)?;
    linker.func_wrap("term", "exit", term::w_exit)?;
    linker.func_wrap("term", "clear", term::w_clear)?;
    linker.func_wrap("term", "move_to", term::w_move_to)?;
    linker.func_wrap("term", "size", term::w_size)?;
    linker.func_wrap("term", "flush", term::w_flush)?;
    linker.func_wrap("term", "read_key", term::w_read_key)?;
    linker.func_wrap("ui", "label", gui::abi::label)?;
    linker.func_wrap("ui", "button", gui::abi::button)?;

    for lib in &manifest.libs {
        wire_lib(&mut linker, &mut store, engine, &join(base, &lib.path), lib)?;
    }
    let app_shared = store.data().shared.clone();
    for bridge in &manifest.bridges {
        wire_bridge(
            &mut linker,
            &mut store,
            engine,
            &join(base, &bridge.path),
            app_shared,
            bridge,
        )?;
    }
    let app_mod = Module::from_file(engine, join(base, &manifest.app.path))?;
    let app_inst = linker.instantiate(&mut store, &app_mod)?;

    Ok(Linked {
        store,
        app_inst,
        run_name: manifest.app.run.clone(),
    })
}
