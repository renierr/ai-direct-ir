//! The Component Model target: assemble, validate, and run a WASM component.
//!
//! Core WAT and components are separate linking domains: a component cannot be
//! satisfied by a Core linker import, and a Core module cannot call a WIT
//! interface. This path is therefore additive. It shares the manifest, the CLI,
//! and the WAT assembler with the Core path, and nothing else.
//!
//! Nothing here is new machinery: the embedded `wat` parser already handles the
//! Component Model text format, and `wasmtime-wasi` already carries WASI 0.2.

use std::path::{Path, PathBuf};

use wasmtime::component::{Component, Func, Instance, Linker, ResourceTable, types::ComponentItem};
use wasmtime::{Engine, Result, Store, StoreContextMut};
use wasmtime_wasi::p2;
use wasmtime_wasi::{FsPerms, I32Exit, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::manifest::Manifest;
use crate::manifest::join;

/// Store state for a component: the WASI 0.2 configuration plus the table that
/// owns the streams, descriptors, and other handles the guest holds.
pub struct Host {
    ctx: WasiCtx,
    table: ResourceTable,
    /// Set while the guest holds the terminal in raw mode, so it is restored
    /// on the way out even if the guest forgets.
    pub term_active: bool,
    /// What the guest drew this frame, in call order. `ai-direct:host/ui`
    /// records rather than renders: the guest runs to completion, then the
    /// GUI runtime replays the list. Empty for every non-GUI component.
    pub ui: Vec<crate::gui::UiCommand>,
    /// Which buttons the user pressed on the frame just drawn. A `button`
    /// call answers from here, so the guest learns about a click on the
    /// frame after it.
    pub ui_clicked: std::collections::HashSet<String>,
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
pub fn link_all(
    engine: &Engine,
    manifest: &Manifest,
    base: &Path,
    env: &crate::cmds::GuestEnv,
) -> Result<Linked> {
    let path = join(base, &manifest.app.path);
    let bytes = std::fs::read(&path)?;
    if !crate::manifest::is_component_binary(&bytes) {
        return Err(wasmtime::Error::msg(format!(
            "{} is a Core WASM module, not a component; author a `(component ...)` \
             source, or lift it with `wasm-tools component new`",
            path.display()
        )));
    }
    let component = Component::new(engine, &bytes)?;

    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();
    // argv[0] is the program name by convention, so a guest's usage message can
    // name itself. Whether argv reaches the guest at all is the host's call, not
    // something WASI decides -- this is the line that makes it so.
    let mut argv = vec![
        Path::new(&manifest.app.path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "app".into()),
    ];
    argv.extend(env.args.iter().cloned());
    builder.args(&argv);
    // Nothing is reachable unless it asks. `wasi:sockets` is linked for every
    // component, so without this grant a `bind` answers `access-denied` --
    // which is the right default, and the same rule the directory grants
    // follow. Name lookup rides along: a program that may open a connection
    // may work out where to open it.
    if manifest.network || env.network {
        // `inherit_network` only settles *which addresses* are allowed; TCP,
        // UDP and name lookup are each disabled by default and gated
        // separately. One grant opens all three -- the boundary already
        // declares nothing an application did not import, so a TCP-only
        // program cannot reach UDP regardless.
        builder.inherit_network();
        builder.allow_tcp(true);
        builder.allow_udp(true);
        builder.allow_ip_name_lookup(true);
    }
    // `--dir <path>` grants one directory, named to the guest exactly as it was
    // written. A tool that reads whatever file it is pointed at needs this, and
    // making it explicit is the point: nothing is readable by default.
    for (dir, write) in &env.dirs {
        let perms = if *write {
            FsPerms::ReadWrite
        } else {
            FsPerms::ReadOnly
        };
        builder.preopened_dir(dir, dir, perms)?;
    }
    if let Some(root) = &manifest.root {
        let guest = manifest.guest.clone().unwrap_or_else(|| {
            Path::new(root)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "root".into())
        });
        builder.preopened_dir(join(base, root), guest, FsPerms::ReadOnly)?;
    }
    // `[[dirs]]` is the general form: any number of directories, each named to
    // the guest and each read-only unless it asks for writes. An application
    // that keeps state -- a database, a cache, a log -- needs one of these.
    for dir in &manifest.dirs {
        let guest = dir.guest.clone().unwrap_or_else(|| dir.path.clone());
        // A manifest path is project-relative, never relative to the shell that
        // launched it: the directory belongs to the application and travels
        // with it through `air dist`. `--dir` is the other anchor, for
        // directories the user is pointing at from their own working directory.
        let host = if Path::new(&dir.path).is_absolute() {
            std::path::PathBuf::from(&dir.path)
        } else {
            base.join(&dir.path)
        };
        let perms = if dir.write {
            // A writable directory is the application's own storage, so create
            // it on first run rather than making every app ship an empty one.
            std::fs::create_dir_all(&host)?;
            FsPerms::ReadWrite
        } else {
            FsPerms::ReadOnly
        };
        builder
            .preopened_dir(&host, guest, perms)
            .map_err(|error| {
                wasmtime::Error::msg(format!(
                    "cannot grant `{}` to the app: {error}",
                    host.display()
                ))
            })?;
    }
    let mut store = Store::new(
        engine,
        Host {
            ctx: builder.build(),
            table: ResourceTable::new(),
            term_active: false,
            ui: Vec::new(),
            ui_clicked: std::collections::HashSet::new(),
        },
    );

    let mut linker = Linker::<Host>::new(engine);
    p2::add_to_linker_sync(&mut linker)?;
    add_term_to_linker(&mut linker)?;
    add_ui_to_linker(&mut linker)?;
    wire_providers(engine, &mut linker, &mut store, &component, manifest, base)?;
    let instance = linker.instantiate(&mut store, &component)?;
    let entry = entry_of(engine, &component, &manifest.app.run)?;
    Ok(Linked {
        store,
        instance,
        entry,
    })
}

/// The project's own terminal capability, offered to components under a WIT
/// interface name instead of the Core `term.*` namespace. A custom host
/// interface is not a WASI question: a component imports one exactly as it
/// imports `wasi:io/streams`, and the harness supplies it here.
///
/// The signatures are not the Core ones transcribed. `term.*` answers every
/// call with an `i32` status a caller almost never reads, and packs the
/// terminal size into one of them; stated in WIT the questions have their own
/// shapes -- `available` and `enter` answer `bool`, `size` answers a pair, and
/// the calls whose failure a program cannot act on answer nothing at all. The
/// harness restores the terminal on the way out however the guest left, so a
/// failed `exit` is not the guest's problem to handle.
const TERM_INTERFACE: &str = "ai-direct:host/term";

fn add_term_to_linker(linker: &mut Linker<Host>) -> Result<()> {
    let mut term = linker.instance(TERM_INTERFACE)?;
    term.func_wrap("enter", |mut store: StoreContextMut<'_, Host>, (): ()| {
        Ok((crate::term::enter(&mut store.data_mut().term_active) == 0,))
    })?;
    term.func_wrap("exit", |mut store: StoreContextMut<'_, Host>, (): ()| {
        crate::term::exit(&mut store.data_mut().term_active);
        Ok(())
    })?;
    term.func_wrap("available", |_: StoreContextMut<'_, Host>, (): ()| {
        Ok((crate::term::available() == 1,))
    })?;
    term.func_wrap("clear", |store: StoreContextMut<'_, Host>, (): ()| {
        crate::term::clear(store.data().term_active);
        Ok(())
    })?;
    term.func_wrap(
        "move-to",
        |store: StoreContextMut<'_, Host>, (x, y): (u32, u32)| {
            crate::term::move_to(store.data().term_active, x as i32, y as i32);
            Ok(())
        },
    )?;
    // A terminal too large to describe in two `u16`s does not exist, but a
    // failed query does: it answers 0x0, which every caller already has to
    // handle as "too small to draw in".
    term.func_wrap("size", |_: StoreContextMut<'_, Host>, (): ()| {
        let packed = crate::term::size();
        let size = if packed < 0 {
            (0u32, 0u32)
        } else {
            ((packed >> 16) as u32, (packed & 0xffff) as u32)
        };
        Ok((size,))
    })?;
    term.func_wrap("flush", |_: StoreContextMut<'_, Host>, (): ()| {
        crate::term::flush();
        Ok(())
    })?;
    // Key codes are the Core namespace's, unchanged: a printable key is its
    // own byte and a named key is `0x100 | <code>`.
    term.func_wrap("read-key", |store: StoreContextMut<'_, Host>, (): ()| {
        Ok((crate::term::read_key(store.data().term_active)? as u32,))
    })?;
    Ok(())
}

/// The project's own immediate-mode UI capability.
///
/// The Core `ui.*` namespace this replaces passed `(ptr, len)` into guest
/// memory, which has no meaning across a component boundary: the two sides do
/// not share a linear memory, and a component may not even have one the host
/// can name. Stated in WIT the signatures need no memory at all --
///
/// ```wit
/// label:  func(text: string);
/// button: func(text: string) -> bool;
/// ```
///
/// -- and the canonical ABI does the copy, the bounds check and the UTF-8
/// validation that the Core wrappers had to do by hand.
const UI_INTERFACE: &str = "ai-direct:host/ui";

fn add_ui_to_linker(linker: &mut Linker<Host>) -> Result<()> {
    let mut ui = linker.instance(UI_INTERFACE)?;
    ui.func_wrap(
        "label",
        |mut store: StoreContextMut<'_, Host>, (text,): (String,)| {
            store.data_mut().ui.push(crate::gui::UiCommand::Label(text));
            Ok(())
        },
    )?;
    ui.func_wrap(
        "button",
        |mut store: StoreContextMut<'_, Host>, (text,): (String,)| {
            // The answer is last frame's click: this frame's is not known
            // until the guest has finished describing what to draw.
            let clicked = store.data().ui_clicked.contains(&text);
            store
                .data_mut()
                .ui
                .push(crate::gui::UiCommand::Button(text));
            Ok((clicked,))
        },
    )?;
    Ok(())
}

/// Instantiate each declared provider component and forward its exported
/// functions into the application's linker.
///
/// This is runtime linking, not build-time composition: the application still
/// imports the provider's interface, and `air` satisfies that import by
/// calling into an already-instantiated provider. It needs no external
/// composer. What it does not do is fuse the two into one distributable
/// component, and handles cannot cross the boundary, because each instance
/// owns its own resource table. Plain values pass through untouched.
///
/// A provider that does not export what the application imports fails here
/// with the entry named, rather than at instantiate as a linker error.
fn wire_providers(
    engine: &Engine,
    linker: &mut Linker<Host>,
    store: &mut Store<Host>,
    app: &Component,
    manifest: &Manifest,
    base: &Path,
) -> Result<()> {
    // Load every provider before wiring any: conformance is checked against
    // the union of what all of them export.
    let mut loaded: Vec<(PathBuf, Component)> = Vec::new();
    for provider in &manifest.providers {
        let path = join(base, &provider.path);
        let bytes = std::fs::read(&path)?;
        if !crate::manifest::is_component_binary(&bytes) {
            return Err(wasmtime::Error::msg(format!(
                "provider `{}` is a Core WASM module, not a component",
                path.display()
            )));
        }
        loaded.push((path, Component::new(engine, &bytes)?));
    }
    check_provider_exports(engine, app, &loaded)?;
    for (path, component) in &loaded {
        // A provider gets WASI and nothing else: it may not depend on the
        // application, and provider-to-provider wiring is not supported yet.
        let mut provider_linker = Linker::<Host>::new(engine);
        p2::add_to_linker_sync(&mut provider_linker)?;
        let instance = provider_linker.instantiate(&mut *store, component)?;

        // Resolve every exported function first, then define them: looking up
        // needs the store, defining needs the linker.
        let mut exported: Vec<(Option<String>, String, Func)> = Vec::new();
        for (name, item) in component.component_type().exports(engine) {
            match item.ty {
                ComponentItem::ComponentFunc(_) => {
                    let func = lookup(&instance, store, None, name, path)?;
                    exported.push((None, name.to_string(), func));
                }
                ComponentItem::ComponentInstance(interface) => {
                    let outer = instance
                        .get_export_index(&mut *store, None, name)
                        .ok_or_else(|| lost(path, name))?;
                    for (func_name, func_item) in interface.exports(engine) {
                        if !matches!(func_item.ty, ComponentItem::ComponentFunc(_)) {
                            continue;
                        }
                        let func = lookup(&instance, store, Some(&outer), func_name, path)?;
                        exported.push((Some(name.to_string()), func_name.to_string(), func));
                    }
                }
                _ => {}
            }
        }
        if exported.is_empty() {
            return Err(wasmtime::Error::msg(format!(
                "provider `{}` exports no functions to wire",
                path.display()
            )));
        }
        for (interface, name, func) in exported {
            let mut target = match &interface {
                Some(interface) => linker.instance(interface)?,
                None => linker.root(),
            };
            target.func_new(&name, move |mut store, _ty, params, results| {
                func.call(&mut store, params, results)
            })?;
        }
    }
    Ok(())
}

/// Fail before linking when a declared provider does not export what the
/// application imports.
///
/// Without this the mismatch surfaces at `linker.instantiate` as a linker
/// error naming the interface but not the `[[providers]]` entry at fault.
/// For every interface at least one provider exports, each function the app
/// imports from that same-named interface must be exported by some provider;
/// otherwise the error names the interface, the missing function, and the
/// entries exporting that interface. Host interfaces never trigger this: no
/// provider exports them, so they stay the linker's business.
fn check_provider_exports(
    engine: &Engine,
    app: &Component,
    providers: &[(PathBuf, Component)],
) -> Result<()> {
    use std::collections::{HashMap, HashSet};

    /// One wanted or offered function: an instance interface, or the root
    /// namespace for a component-level function.
    type At = (Option<String>, String);
    let mut wanted: HashSet<At> = HashSet::new();
    for (name, item) in app.component_type().imports(engine) {
        match &item.ty {
            ComponentItem::ComponentFunc(_) => {
                wanted.insert((None, name.to_string()));
            }
            ComponentItem::ComponentInstance(interface) => {
                for (func, exported) in interface.exports(engine) {
                    if matches!(exported.ty, ComponentItem::ComponentFunc(_)) {
                        wanted.insert((Some(name.to_string()), func.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
    // Which providers export each interface at all, for attribution.
    let mut claimants: HashMap<Option<String>, Vec<String>> = HashMap::new();
    let mut offered: HashSet<At> = HashSet::new();
    for (path, provider) in providers {
        let display = path.display().to_string();
        for (name, item) in provider.component_type().exports(engine) {
            match &item.ty {
                ComponentItem::ComponentFunc(_) => {
                    claimants.entry(None).or_default().push(display.clone());
                    offered.insert((None, name.to_string()));
                }
                ComponentItem::ComponentInstance(interface) => {
                    let at = Some(name.to_string());
                    claimants
                        .entry(at.clone())
                        .or_default()
                        .push(display.clone());
                    for (func, exported) in interface.exports(engine) {
                        if matches!(exported.ty, ComponentItem::ComponentFunc(_)) {
                            offered.insert((at.clone(), func.to_string()));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut missing: Vec<At> = wanted
        .into_iter()
        .filter(|at| claimants.contains_key(&at.0) && !offered.contains(at))
        .collect();
    missing.sort();
    let Some((at, func)) = missing.into_iter().next() else {
        return Ok(());
    };
    let mut sources = claimants[&at].clone();
    sources.sort();
    sources.dedup();
    let from = match &at {
        Some(interface) => format!("interface `{interface}`"),
        None => "the root namespace".to_string(),
    };
    let who = sources
        .iter()
        .map(|source| format!("`{source}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let verb = if sources.len() == 1 {
        "provider"
    } else {
        "providers"
    };
    Err(wasmtime::Error::msg(format!(
        "{verb} {who} export {from} without `{func}`, which the application imports"
    )))
}

fn lost(path: &Path, name: &str) -> wasmtime::Error {
    wasmtime::Error::msg(format!(
        "provider `{}` lost export `{name}`",
        path.display()
    ))
}

fn lookup(
    instance: &Instance,
    store: &mut Store<Host>,
    outer: Option<&wasmtime::component::ComponentExportIndex>,
    name: &str,
    path: &Path,
) -> Result<Func> {
    let index = instance
        .get_export_index(&mut *store, outer, name)
        .ok_or_else(|| lost(path, name))?;
    instance
        .get_func(&mut *store, &index)
        .ok_or_else(|| lost(path, name))
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

/// Validate a component without executing it.
pub fn check(engine: &Engine, manifest_path: &str, manifest: &Manifest, base: &Path) -> Result<()> {
    // A check never runs the guest, so it needs no guest arguments.
    let mut linked = link_all(engine, manifest, base, &crate::cmds::GuestEnv::default())?;
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
pub fn run(
    engine: &Engine,
    manifest: &Manifest,
    base: &Path,
    env: &crate::cmds::GuestEnv,
) -> Result<()> {
    let mut linked = link_all(engine, manifest, base, env)?;
    let outcome = match &linked.entry {
        Entry::Command { .. } => command_func(&mut linked).and_then(|func| {
            let (result,) = exit_aware(func.call(&mut linked.store, ()))?;
            result.map_err(|()| wasmtime::Error::msg("component run failed"))
        }),
        Entry::Function { .. } => plain_func(&mut linked).and_then(|func| {
            exit_aware(func.call(&mut linked.store, ()))?;
            Ok(())
        }),
    };
    // A guest that took the terminal does not get to keep it, however it left.
    crate::term::restore_flag(&mut linked.store.data_mut().term_active);
    outcome
}

/// `wasi:cli/exit` unwinds through Wasmtime as an error carrying the status.
/// That is a finished program, not a crash, so it must not print a backtrace.
fn exit_aware<T>(result: Result<T>) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(exit) = error.downcast_ref::<I32Exit>() {
                std::process::exit(exit.0);
            }
            Err(error)
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

pub fn plain_func(linked: &mut Linked) -> Result<wasmtime::component::TypedFunc<(), ()>> {
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
