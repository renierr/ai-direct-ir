//! The Component Model target: assemble, validate, and run a WASM component.
//!
//! Core WAT and components are separate linking domains: a component cannot be
//! satisfied by a Core linker import, and a Core module cannot call a WIT
//! interface. This path is therefore additive. It shares the manifest, the CLI,
//! and the WAT assembler with the Core path, and nothing else.
//!
//! Nothing here is new machinery: the embedded `wat` parser already handles the
//! Component Model text format, and `wasmtime-wasi` already carries WASI 0.2.

use std::path::Path;

use wasmtime::component::{Component, Instance, Linker, ResourceTable};
use wasmtime::{Engine, Result, Store};
use wasmtime_wasi::p2;
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::link::join;
use crate::manifest::Manifest;

/// Store state for a component: the WASI 0.2 configuration plus the table that
/// owns the streams, descriptors, and other handles the guest holds.
pub struct Host {
    ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for Host {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

/// The manifest `run` value that selects a `wasi:cli/command` entry point.
pub const COMMAND_RUN: &str = "wasi:cli/run";

/// WASI versions its interface names, so the exact patch version is discovered
/// from the component rather than assumed by the harness.
const COMMAND_PREFIX: &str = "wasi:cli/run@";

/// How the harness enters a component.
pub enum Entry {
    /// `run: func() -> result` inside the component's `wasi:cli/run` export.
    Command { instance: String },
    /// A component-level `func()` named by the manifest.
    Function { name: String },
}

pub struct Linked {
    pub store: Store<Host>,
    pub instance: Instance,
    pub entry: Entry,
}

/// Load, link, and instantiate a component, stopping before execution.
/// Instantiation is what proves every WASI import the component declares is
/// actually satisfied, exactly as the Core path relies on it.
pub fn link_all(engine: &Engine, manifest: &Manifest, base: &Path) -> Result<Linked> {
    if !manifest.libs.is_empty() || !manifest.bridges.is_empty() {
        return Err(wasmtime::Error::msg(
            "a component app cannot declare [[libs]] or [[bridges]]: those are Core WASM \
             mechanisms. Compose provider components instead.",
        ));
    }
    let path = join(base, &manifest.app.path);
    let bytes = std::fs::read(&path)?;
    if !is_component(&bytes) {
        return Err(wasmtime::Error::msg(format!(
            "{} is a Core WASM module, not a component; \
             use target = \"native\" or author a `(component ...)` source",
            path.display()
        )));
    }
    let component = Component::new(engine, &bytes)?;

    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();
    if let Some(root) = &manifest.root {
        let guest = manifest.guest.clone().unwrap_or_else(|| {
            Path::new(root)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "root".into())
        });
        builder.preopened_dir(join(base, root), guest, FsPerms::ReadOnly)?;
    }
    let mut store = Store::new(
        engine,
        Host {
            ctx: builder.build(),
            table: ResourceTable::new(),
        },
    );

    let mut linker = Linker::<Host>::new(engine);
    p2::add_to_linker_sync(&mut linker)?;
    let instance = linker.instantiate(&mut store, &component)?;
    let entry = entry_of(engine, &component, &manifest.app.run)?;
    Ok(Linked {
        store,
        instance,
        entry,
    })
}

/// Resolve the manifest's `run` value against the component's actual exports.
fn entry_of(engine: &Engine, component: &Component, run: &str) -> Result<Entry> {
    if run != COMMAND_RUN {
        let found = component
            .component_type()
            .exports(engine)
            .any(|(name, _)| name == run);
        if !found {
            return Err(wasmtime::Error::msg(format!(
                "component has no export `{run}`"
            )));
        }
        return Ok(Entry::Function {
            name: run.to_string(),
        });
    }
    let instance = component
        .component_type()
        .exports(engine)
        .map(|(name, _)| name.to_string())
        .find(|name| name.starts_with(COMMAND_PREFIX))
        .ok_or_else(|| {
            wasmtime::Error::msg(format!(
                "run = \"{COMMAND_RUN}\" needs a `{COMMAND_PREFIX}<version>` export; \
                 the component exports none"
            ))
        })?;
    Ok(Entry::Command { instance })
}

/// A component binary starts with the WASM preamble and layer 1; a Core module
/// declares layer 0. Reading it here keeps the error specific.
fn is_component(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..4] == *b"\0asm" && bytes[4..6] == [0x0d, 0x00]
}

/// Validate a component without executing it.
pub fn check(engine: &Engine, manifest_path: &str, manifest: &Manifest, base: &Path) -> Result<()> {
    let mut linked = link_all(engine, manifest, base)?;
    let described = match &linked.entry {
        Entry::Command { instance } => format!("run `{instance}`: command entry ok"),
        Entry::Function { name } => format!("run `{name}`: signature ok"),
    };
    match &linked.entry {
        Entry::Command { .. } => drop(command_func(&mut linked)?),
        Entry::Function { .. } => drop(plain_func(&mut linked)?),
    }
    println!("{described}");
    println!("check {manifest_path}: component instantiated, all imports satisfied");
    Ok(())
}

/// Execute a component. A command entry's `Err` result is a failed run, which
/// the process reports as a non-zero exit exactly like a Core `proc_exit`.
pub fn run(engine: &Engine, manifest: &Manifest, base: &Path) -> Result<()> {
    let mut linked = link_all(engine, manifest, base)?;
    match &linked.entry {
        Entry::Command { .. } => {
            let func = command_func(&mut linked)?;
            let (result,) = func.call(&mut linked.store, ())?;
            result.map_err(|()| wasmtime::Error::msg("component run failed"))
        }
        Entry::Function { .. } => {
            let func = plain_func(&mut linked)?;
            func.call(&mut linked.store, ())?;
            Ok(())
        }
    }
}

type CommandFunc = wasmtime::component::TypedFunc<(), (std::result::Result<(), ()>,)>;

fn command_func(linked: &mut Linked) -> Result<CommandFunc> {
    let Entry::Command { instance } = &linked.entry else {
        return Err(wasmtime::Error::msg("not a command entry"));
    };
    let outer = linked
        .instance
        .get_export_index(&mut linked.store, None, instance)
        .ok_or_else(|| wasmtime::Error::msg(format!("component lost export `{instance}`")))?;
    let index = linked
        .instance
        .get_export_index(&mut linked.store, Some(&outer), "run")
        .ok_or_else(|| wasmtime::Error::msg(format!("`{instance}` has no `run` function")))?;
    let func = linked
        .instance
        .get_func(&mut linked.store, &index)
        .ok_or_else(|| wasmtime::Error::msg(format!("`{instance}#run` is not a function")))?;
    func.typed::<(), (std::result::Result<(), ()>,)>(&linked.store)
        .map_err(|e| {
            wasmtime::Error::msg(format!("`{instance}#run` must be `func() -> result`: {e}"))
        })
}

fn plain_func(linked: &mut Linked) -> Result<wasmtime::component::TypedFunc<(), ()>> {
    let Entry::Function { name } = &linked.entry else {
        return Err(wasmtime::Error::msg("not a function entry"));
    };
    let func = linked
        .instance
        .get_func(&mut linked.store, name.as_str())
        .ok_or_else(|| wasmtime::Error::msg(format!("`{name}` is not a function")))?;
    func.typed::<(), ()>(&linked.store)
        .map_err(|e| wasmtime::Error::msg(format!("`{name}` must be `func()`: {e}")))
}
