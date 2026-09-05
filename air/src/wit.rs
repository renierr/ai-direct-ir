//! Derive component-WAT imports from WIT text instead of hardcoding them.
//!
//! Declaring a WASI interface by hand means transcribing its whole type graph
//! into the component text format: every enum case, every flag, every method
//! signature, with the exported type id in every position. `sha256sum` showed
//! what that costs. The WIT ships with Wasmtime (vendored under `air/wit/`),
//! so `air` parses it with `wit-parser` and emits the same WAT an author
//! could have written.
//!
//! Scope is deliberately one capability: `wasi:filesystem`, the interface the
//! repository actually consumes. The emitter itself is generic over the WIT
//! type system (resources, records, variants, enums, flags, tuples, options,
//! results, lists, borrows, owns); teaching `;; @wasi` a new interface is
//! vendoring its WIT and wiring one more entry point, not transcribing it.
//!
//! The WIT declares 29 filesystem functions and the whole type graph behind
//! them. An application names the handful it calls, as `(import "fs" "...")`
//! lines, and the boundary declares exactly those plus the types their
//! signatures reach — the same rule as the rest of `;; @wasi`, which emits one
//! import per requested capability and nothing else.
//!
//! The generated names extend the boundary ABI documented in `boundary.rs`:
//!
//! - `$fs-clock` — the `wasi:clocks/wall-clock` import, `datetime` only.
//! - `$fs-types` — the `wasi:filesystem/types` import.
//! - `$fs-pre` — the `wasi:filesystem/preopens` import.
//! - `$fs` — the core instance gathering every lowered filesystem function.
//!   An application links it like the WASI one: `(with "fs" (instance $fs))`
//!   and imports the names `import_name` derives — `"descriptor.open-at"`,
//!   `"get-directories"`.
//!
//! Alongside the functions, each exported resource offers `<resource>.drop`
//! (`"descriptor.drop"`, `"tcp-socket.drop"`). It is a canonical builtin
//! rather than a WIT function, but a handle is the one thing the canonical ABI
//! cannot release on the application's behalf, so a program that opens handles
//! in a loop needs it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use wit_parser::{FunctionKind, InterfaceId, Resolve, Type, TypeDefKind, TypeId};

const IO_WIT: &str = include_str!("../wit/wasi-0.2.12/io.wit");
const CLOCKS_WIT: &str = include_str!("../wit/wasi-0.2.12/clocks.wit");
const FILESYSTEM_WIT: &str = include_str!("../wit/wasi-0.2.12/filesystem.wit");
const SOCKETS_WIT: &str = include_str!("../wit/wasi-0.2.12/sockets.wit");

/// One entry of a capability's core instance: the name the application imports
/// (see `import_name`), the Core function it is, and the `canon` definition
/// behind it -- `lower` for a WIT function, `resource.drop` for a handle
/// release. The `canon` text names `$memory` and `$realloc`, which
/// `boundary.rs` defines and documents; the generated WAT is written against
/// that ABI the same way it is written against `$istream`.
pub struct Lower {
    pub export: String,
    pub core: String,
    pub canon: String,
}

/// One generated boundary: import text plus aliases, and the lowering list
/// `boundary.rs` turns into a core instance.
pub struct Generated {
    pub wat: String,
    pub lowers: Vec<Lower>,
}

/// A capability `;; @wasi` can generate: which WIT interfaces it declares, and
/// the names the harness gives them.
pub struct Capability {
    /// The core instance an application links: `(with "fs" (instance $fs))`.
    pub instance: &'static str,
    /// Prefix for every generated WAT identifier.
    prefix: &'static str,
    /// The WIT package this capability is, for error messages and the header.
    package: &'static str,
    /// The vendored file that is its source of truth.
    source: &'static str,
    /// Interfaces in dependency order: one may only `use` types from an
    /// interface declared before it.
    interfaces: &'static [Interface],
}

struct Interface {
    /// Fully-qualified WIT interface name.
    wit: &'static str,
    /// Suffix for this interface's instance variable: `$fs-types`.
    var: &'static str,
    /// False when the interface is here only as a type source for a later one.
    /// `wasi:filesystem` needs `datetime` from `wasi:clocks/wall-clock`; that
    /// does not make reading the clock part of the filesystem capability.
    functions: bool,
}

/// `;; @wasi filesystem`. `wasi:clocks/wall-clock` supplies the one type
/// (`datetime`) the filesystem graph needs beyond `wasi:io`.
pub const FILESYSTEM: Capability = Capability {
    instance: "fs",
    prefix: "$fs",
    package: "wasi:filesystem",
    source: "air/wit/wasi-0.2.12/filesystem.wit",
    interfaces: &[
        Interface {
            wit: "wasi:clocks/wall-clock@0.2.12",
            var: "clock",
            functions: false,
        },
        Interface {
            wit: "wasi:filesystem/types@0.2.12",
            var: "types",
            functions: true,
        },
        Interface {
            wit: "wasi:filesystem/preopens@0.2.12",
            var: "pre",
            functions: true,
        },
    ],
};

/// `;; @wasi sockets`. `wasi:io/poll` comes along with functions, not just
/// types: `pollable.block` is how a guest waits for a connection, and there is
/// no synchronous accept in WASI 0.2. `wasi:clocks/monotonic-clock` supplies
/// `duration` for the keep-alive setters and nothing else.
pub const SOCKETS: Capability = Capability {
    instance: "net",
    prefix: "$net",
    package: "wasi:sockets",
    source: "air/wit/wasi-0.2.12/sockets.wit",
    interfaces: &[
        Interface {
            wit: "wasi:clocks/monotonic-clock@0.2.12",
            var: "clock",
            functions: false,
        },
        Interface {
            wit: "wasi:io/poll@0.2.12",
            var: "poll",
            functions: true,
        },
        Interface {
            wit: "wasi:sockets/network@0.2.12",
            var: "network",
            functions: true,
        },
        Interface {
            wit: "wasi:sockets/instance-network@0.2.12",
            var: "inst",
            functions: true,
        },
        Interface {
            wit: "wasi:sockets/ip-name-lookup@0.2.12",
            var: "lookup",
            functions: true,
        },
        Interface {
            wit: "wasi:sockets/tcp@0.2.12",
            var: "tcp",
            functions: true,
        },
        Interface {
            wit: "wasi:sockets/tcp-create-socket@0.2.12",
            var: "tcp-new",
            functions: true,
        },
        Interface {
            wit: "wasi:sockets/udp@0.2.12",
            var: "udp",
            functions: true,
        },
        Interface {
            wit: "wasi:sockets/udp-create-socket@0.2.12",
            var: "udp-new",
            functions: true,
        },
    ],
};

/// Parse the vendored WASI WIT and emit `capability`'s boundary for the names
/// `used` — the ones the application imports from its instance.
///
/// Every type and signature comes from that WIT. Only the instance variable
/// names (`$fs-types`, `$net-tcp`, ...) are the harness's own ABI, exactly like
/// `$mem` and `$wasi`.
pub fn generate(capability: &Capability, used: &BTreeSet<String>) -> wasmtime::Result<Generated> {
    let resolve = resolve()?;
    let ids = capability
        .interfaces
        .iter()
        .map(|iface| interface(&resolve, iface.wit))
        .collect::<wasmtime::Result<Vec<_>>>()?;

    // What the application imports decides what the boundary declares. An
    // unknown name is an error rather than an import that fails to link with
    // the WIT's whole catalogue as the only hint.
    let catalog = catalog(&resolve, capability, &ids)?;
    let mut wanted: HashSet<String> = HashSet::new();
    let mut dropped: HashSet<TypeId> = HashSet::new();
    for name in used {
        match catalog.get(name) {
            Some((_, Entry::Func(wit_name))) => {
                wanted.insert(wit_name.clone());
            }
            Some((_, Entry::Drop(resource))) => {
                dropped.insert(*resource);
            }
            None => {
                return Err(wasmtime::Error::msg(format!(
                    "`(import \"{}\" \"{name}\" ...)` names no `{}` function; \
                     the boundary exports only what `{}` declares, plus \
                     `<resource>.drop` for each resource it exports",
                    capability.instance, capability.package, capability.source
                )));
            }
        }
    }
    // Types the wanted signatures reach, and nothing else: the 37-case
    // `error-code` enums and the `descriptor-stat` record cost real bytes in
    // every artifact that declares them. A dropped resource keeps itself: the
    // application may release a handle it never names in a signature.
    let mut keep: HashSet<TypeId> = dropped.clone();
    for id in &ids {
        for (wit_name, func) in &resolve.interfaces[*id].functions {
            if wanted.contains(wit_name) {
                reach_func(&resolve, func, &mut keep);
            }
        }
    }

    let heading = format!(
        "  ;; --- {} boundary, generated from WIT ",
        capability.package
    );
    let mut out = heading.clone();
    out.push_str(&"-".repeat(78usize.saturating_sub(heading.len())));
    out.push_str(&format!(
        "\n  ;; `{}` is the source of truth.\n",
        capability.source
    ));

    // `wasi:io` streams arrive through the `@wasi` streams boundary, which
    // every generated capability implies (see `Boundary::streams`). Refer to
    // those names directly, the way the hand-written boundary did.
    let mut outer: HashMap<TypeId, String> = HashMap::new();
    for (name, wat) in [
        ("input-stream", "$istream"),
        ("output-stream", "$ostream"),
        ("error", "$error"),
    ] {
        outer.insert(canon(&resolve, io_type(&resolve, name)?), wat.to_string());
    }

    // Which instance exports each type already declared, so a later interface
    // can alias it instead of redeclaring the graph behind it. `wasi:sockets`
    // makes this pay: one `error-code` enum serves `tcp`, `udp` and lookups.
    let mut declared: HashMap<TypeId, (String, String)> = HashMap::new();
    let mut lowers = Vec::new();
    for (iface, id) in capability.interfaces.iter().zip(&ids) {
        let kept_types: Vec<(String, TypeId)> = resolve.interfaces[*id]
            .types
            .iter()
            .filter(|(_, type_id)| keep.contains(type_id))
            .map(|(name, type_id)| (name.clone(), *type_id))
            .collect();
        let kept_funcs = resolve.interfaces[*id]
            .functions
            .keys()
            .any(|wit_name| wanted.contains(wit_name));
        if kept_types.is_empty() && !kept_funcs {
            continue;
        }
        let var = format!("{}-{}", capability.prefix, iface.var);
        // An inline instance export only takes `eq`/`sub`, so a type this
        // interface `use`s has to be bound to an outer name before the import
        // that references it.
        let mut foreign = outer.clone();
        for (name, type_id) in &kept_types {
            let TypeDefKind::Type(Type::Id(inner)) = &resolve.types[*type_id].kind else {
                continue;
            };
            let target = canon(&resolve, *inner);
            if foreign.contains_key(&target) {
                continue;
            }
            let Some((source_var, source_name)) = declared.get(&target) else {
                continue;
            };
            let alias = format!("{var}-{name}");
            out.push_str(&format!(
                "  (alias export {source_var} \"{source_name}\" (type {alias}))\n"
            ));
            foreign.insert(target, alias);
        }
        let mut emitter = Emitter::new(&resolve, &var, foreign);
        lowers.extend(emitter.emit_interface(*id, &var, &keep, &wanted, &dropped)?);
        out.push_str(&emitter.finish_instance(&var, iface.wit));
        for alias in &emitter.aliases {
            out.push_str(alias);
            out.push('\n');
        }
        out.push('\n');
        for (name, type_id) in &kept_types {
            declared
                .entry(canon(&resolve, *type_id))
                .or_insert((var.clone(), name.clone()));
        }
    }
    Ok(Generated { wat: out, lowers })
}

/// The vendored WASI WIT, parsed. Pushed in dependency order.
fn resolve() -> wasmtime::Result<Resolve> {
    let mut resolve = Resolve::new();
    for (path, contents) in [
        ("wasi-0.2.12/io.wit", IO_WIT),
        ("wasi-0.2.12/clocks.wit", CLOCKS_WIT),
        ("wasi-0.2.12/filesystem.wit", FILESYSTEM_WIT),
        ("wasi-0.2.12/sockets.wit", SOCKETS_WIT),
    ] {
        resolve.push_source(path, contents).map_err(|error| {
            wasmtime::Error::msg(format!("invalid vendored WIT `{path}`: {error:#}"))
        })?;
    }
    Ok(resolve)
}

/// What one importable name of a capability denotes.
enum Entry {
    /// A WIT function, by its export key (`[method]tcp-socket.accept`).
    Func(String),
    /// A resource whose handle the application wants to release.
    Drop(TypeId),
}

/// Every name `capability` offers, keyed as it appears in the flat instance
/// namespace. Two interfaces must not offer the same name: the application
/// imports one instance.
///
/// Alongside the functions, every exported resource offers `<resource>.drop`.
/// A handle is the one thing a component holds that the canonical ABI cannot
/// release for it, and `resource.drop` is a canonical builtin rather than a
/// WIT function, so it has no export key of its own -- but it reads like a
/// method and is named like one.
fn catalog(
    resolve: &Resolve,
    capability: &Capability,
    ids: &[InterfaceId],
) -> wasmtime::Result<BTreeMap<String, (InterfaceId, Entry)>> {
    let mut catalog: BTreeMap<String, (InterfaceId, Entry)> = BTreeMap::new();
    let mut insert = |name: String, id: InterfaceId, entry: Entry| match catalog.entry(name) {
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert((id, entry));
            Ok(())
        }
        std::collections::btree_map::Entry::Occupied(slot) => Err(wasmtime::Error::msg(format!(
            "`{}` and `{}` both export `{}`; cannot share one `${}` namespace",
            resolve.id_of(slot.get().0).unwrap_or_default(),
            resolve.id_of(id).unwrap_or_default(),
            slot.key(),
            capability.instance,
        ))),
    };
    for (iface, id) in capability.interfaces.iter().zip(ids) {
        if !iface.functions {
            continue;
        }
        for wit_name in resolve.interfaces[*id].functions.keys() {
            insert(
                import_name(wit_name).to_string(),
                *id,
                Entry::Func(wit_name.clone()),
            )?;
        }
        for (name, type_id) in &resolve.interfaces[*id].types {
            if matches!(resolve.types[*type_id].kind, TypeDefKind::Resource) {
                insert(format!("{name}.drop"), *id, Entry::Drop(*type_id))?;
            }
        }
    }
    Ok(catalog)
}

/// The name an application imports a WIT function under.
///
/// A WIT export key already qualifies a method by its resource
/// (`[method]descriptor.open-at`); dropping the bracketed kind leaves a name
/// that is unique across a whole package and needs no further rule:
/// `descriptor.open-at`, `tcp-socket.subscribe`, `get-directories`. Bare short
/// names would not do — `wasi:sockets` has five different `subscribe`s.
fn import_name(wit_name: &str) -> &str {
    match wit_name.split_once(']') {
        Some((kind, rest)) if kind.starts_with('[') => rest,
        _ => wit_name,
    }
}

/// Every type one function's signature reaches, transitively. A resource's
/// methods are not part of its type graph, so keeping a resource does not drag
/// its interface back in.
fn reach_func(resolve: &Resolve, func: &wit_parser::Function, out: &mut HashSet<TypeId>) {
    if let FunctionKind::Method(resource) = &func.kind {
        reach_id(resolve, *resource, out);
    }
    for param in &func.params {
        reach(resolve, &param.ty, out);
    }
    if let Some(ty) = &func.result {
        reach(resolve, ty, out);
    }
}

fn reach(resolve: &Resolve, ty: &Type, out: &mut HashSet<TypeId>) {
    if let Type::Id(id) = ty {
        reach_id(resolve, *id, out);
    }
}

/// Both the id as written and everything behind it: a signature may name a
/// `use` alias, and `emit_type` is asked about the alias, not the original.
fn reach_id(resolve: &Resolve, id: TypeId, out: &mut HashSet<TypeId>) {
    if !out.insert(id) {
        return;
    }
    match &resolve.types[id].kind {
        TypeDefKind::Record(record) => {
            for field in &record.fields {
                reach(resolve, &field.ty, out);
            }
        }
        TypeDefKind::Tuple(tuple) => {
            for ty in &tuple.types {
                reach(resolve, ty, out);
            }
        }
        TypeDefKind::Variant(variant) => {
            for case in &variant.cases {
                if let Some(ty) = &case.ty {
                    reach(resolve, ty, out);
                }
            }
        }
        TypeDefKind::Result(result) => {
            for ty in [result.ok.as_ref(), result.err.as_ref()]
                .into_iter()
                .flatten()
            {
                reach(resolve, ty, out);
            }
        }
        TypeDefKind::Option(ty) | TypeDefKind::List(ty) | TypeDefKind::Type(ty) => {
            reach(resolve, ty, out)
        }
        TypeDefKind::Handle(wit_parser::Handle::Own(inner))
        | TypeDefKind::Handle(wit_parser::Handle::Borrow(inner)) => reach_id(resolve, *inner, out),
        // Enums, flags and resources have no operands; anything else the
        // emitter rejects when it reaches `typedef_body`.
        _ => {}
    }
}

fn interface(resolve: &Resolve, name: &str) -> wasmtime::Result<InterfaceId> {
    resolve
        .interfaces
        .iter()
        .find(|(id, _)| resolve.id_of(*id).as_deref() == Some(name))
        .map(|(id, _)| id)
        .ok_or_else(|| wasmtime::Error::msg(format!("vendored WIT has no interface `{name}`")))
}

fn io_type(resolve: &Resolve, name: &str) -> wasmtime::Result<TypeId> {
    // `input-stream` and `output-stream` live on `streams` itself; `error`
    // lives on `wasi:io/error`.
    for candidate in ["wasi:io/streams@0.2.12", "wasi:io/error@0.2.12"] {
        let id = interface(resolve, candidate)?;
        if let Some(type_id) = resolve.interfaces[id].types.get(name) {
            return Ok(*type_id);
        }
    }
    Err(wasmtime::Error::msg(format!(
        "wasi:io has no type `{name}`"
    )))
}

/// Follow `use` alias chains to the defining typedef. A reference to
/// `input-stream` may name the alias or the original; both denote one type.
fn canon(resolve: &Resolve, mut id: TypeId) -> TypeId {
    let mut seen = 0;
    while let TypeDefKind::Type(Type::Id(inner)) = &resolve.types[id].kind {
        id = *inner;
        seen += 1;
        if seen > 100 {
            break;
        }
    }
    id
}

/// WIT types and functions to component-WAT, under the harness's naming
/// scheme. One `Emitter` covers one imported instance.
struct Emitter<'a> {
    resolve: &'a Resolve,
    prefix: String,
    /// Locally emitted types: `TypeId` to `$name`.
    names: HashMap<TypeId, String>,
    /// Foreign types (`use`d from another interface): `TypeId` to the WAT
    /// name that already denotes them (`$istream`, `$fs-datetime-x`, ...).
    /// They are declared `(eq ...)` and referenced directly.
    foreign: HashMap<TypeId, String>,
    /// Instance currently being emitted; alias source.
    instance: String,
    /// Type declaration plus export lines for the current instance, in
    /// dependency order (each declaration immediately before its export).
    type_lines: Vec<String>,
    /// Function export lines for the current instance, in order.
    func_lines: Vec<String>,
    /// `(alias export ...)` lines, emitted after the instance.
    aliases: Vec<String>,
    counter: usize,
}

impl<'a> Emitter<'a> {
    fn new(resolve: &'a Resolve, prefix: &str, foreign: HashMap<TypeId, String>) -> Self {
        Self {
            resolve,
            prefix: prefix.to_string(),
            names: HashMap::new(),
            foreign,
            instance: String::new(),
            type_lines: Vec::new(),
            func_lines: Vec::new(),
            aliases: Vec::new(),
            counter: 0,
        }
    }

    /// Emit one imported instance: every type the WIT interface exports, then
    /// every function, collecting the component-level aliases for lowering.
    /// `keep`, `wanted` and `dropped` narrow the interface to what an
    /// application asked for: only the named functions, the resources it
    /// releases, and only the types their signatures reach. Interface order is
    /// preserved either way, and WIT declares before use, so the kept types
    /// still arrive in dependency order.
    fn emit_interface(
        &mut self,
        id: InterfaceId,
        instance: &str,
        keep: &HashSet<TypeId>,
        wanted: &HashSet<String>,
        dropped: &HashSet<TypeId>,
    ) -> wasmtime::Result<Vec<Lower>> {
        self.instance = instance.to_string();
        // Types first, in interface order: WIT declares before use.
        let type_names: Vec<(String, TypeId)> = self.resolve.interfaces[id]
            .types
            .iter()
            .filter(|(_, type_id)| keep.contains(type_id))
            .map(|(name, type_id)| (name.clone(), *type_id))
            .collect();
        for (name, type_id) in &type_names {
            self.emit_type(id, name, *type_id)?;
        }
        // Functions in interface order.
        let funcs: Vec<(String, wit_parser::Function)> = self.resolve.interfaces[id]
            .functions
            .iter()
            .filter(|(name, _)| wanted.contains(*name))
            .map(|(name, func)| (name.clone(), func.clone()))
            .collect();
        let mut lowers = Vec::with_capacity(funcs.len());
        for (wit_name, func) in &funcs {
            lowers.push(self.emit_func(id, wit_name, func)?);
        }
        // A resource this interface defines and the application releases. Only
        // the defining interface emits it: elsewhere the same resource is a
        // `use` alias, whose `TypeId` is not the one `dropped` holds.
        for (name, type_id) in &type_names {
            if dropped.contains(type_id) {
                lowers.push(self.emit_drop(name));
            }
        }
        Ok(lowers)
    }

    /// Emit the release of one resource: alias the instance's exported type
    /// out, then bind `resource.drop` on it. Unlike a lowering this takes no
    /// memory -- a handle is one `i32` and nothing crosses linear memory.
    fn emit_drop(&mut self, name: &str) -> Lower {
        let ty = format!("{}-{name}", self.prefix);
        self.aliases.push(format!(
            "  (alias export {} \"{name}\" (type {ty}))",
            self.instance
        ));
        Lower {
            export: format!("{name}.drop"),
            core: format!("{ty}-drop-l"),
            canon: format!("resource.drop {ty}"),
        }
    }

    /// Assemble the `(import ...)` for everything emitted so far. Each
    /// type's declaration immediately precedes its export: signatures must
    /// reference the exported id, so the export has to be bound before
    /// anything that uses it.
    fn finish_instance(&self, instance: &str, import: &str) -> String {
        let mut out = format!("  (import \"{import}\" (instance {instance}\n");
        for line in &self.type_lines {
            out.push_str(line);
            out.push('\n');
        }
        for line in &self.func_lines {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("    ))\n");
        out
    }

    fn local(&mut self, stem: &str) -> String {
        self.counter += 1;
        // WIT names are kebab-case; WAT identifiers take them as-is.
        format!("{}-{stem}-t{}", self.prefix, self.counter)
    }

    /// The WAT name denoting `type_id`: foreign names pass through, local
    /// ones must already be emitted. `use` aliases canonicalize to the type
    /// they denote, so the alias and the original share one name.
    fn name_of(&self, type_id: TypeId) -> wasmtime::Result<String> {
        let id = canon(self.resolve, type_id);
        if let Some(name) = self.foreign.get(&id) {
            return Ok(name.clone());
        }
        // The alias itself may already be mapped (a foreign `use` declared
        // `(eq ...)` above).
        if let Some(name) = self.names.get(&type_id) {
            return Ok(name.clone());
        }
        self.names.get(&id).cloned().ok_or_else(|| {
            let def = &self.resolve.types[type_id];
            wasmtime::Error::msg(format!(
                "WIT type {:?} (name {:?}, kind {}) used before declaration",
                type_id,
                def.name,
                def.kind.as_str()
            ))
        })
    }

    /// Emit one named type of the current interface.
    fn emit_type(
        &mut self,
        owner: InterfaceId,
        name: &str,
        type_id: TypeId,
    ) -> wasmtime::Result<()> {
        let def = &self.resolve.types[type_id];
        // A `use` of a foreign type: declare the equality, reference the
        // outer name directly from here on.
        if let TypeDefKind::Type(Type::Id(inner)) = &def.kind {
            let inner_def = &self.resolve.types[canon(self.resolve, *inner)];
            if inner_def.owner != wit_parser::TypeOwner::Interface(owner) {
                let outer = self.name_of(*inner)?;
                self.type_lines
                    .push(format!("    (export \"{name}\" (type (eq {outer})))"));
                // Later references to the alias mean the outer type.
                self.names.insert(type_id, outer);
                return Ok(());
            }
        }
        if matches!(def.kind, TypeDefKind::Resource) {
            let wat = self.local(name);
            self.type_lines.push(format!(
                "    (export \"{name}\" (type {wat} (sub resource)))"
            ));
            self.names.insert(type_id, wat);
            return Ok(());
        }
        // An inline instance export only takes `eq`/`sub`, so structural
        // types declare first and export the equality — the exact pattern the
        // hand-written boundary used. Signatures reference the exported id,
        // and the export is bound before anything that uses it.
        let body = self.typedef_body(type_id)?;
        let local = self.local(name);
        let exported = format!("{local}x");
        self.type_lines.push(format!("    (type {local} {body})"));
        self.type_lines.push(format!(
            "    (export \"{name}\" (type {exported} (eq {local})))"
        ));
        self.names.insert(type_id, exported);
        Ok(())
    }

    /// The WAT spelling of a type definition body (no export wrapper).
    fn typedef_body(&mut self, type_id: TypeId) -> wasmtime::Result<String> {
        let def = self.resolve.types[type_id].clone();
        Ok(match &def.kind {
            TypeDefKind::Record(record) => {
                let mut fields = String::new();
                for field in &record.fields {
                    fields.push_str(&format!(
                        " (field \"{}\" {})",
                        field.name,
                        self.value(&field.ty)?
                    ));
                }
                format!("(record{fields})")
            }
            TypeDefKind::Flags(flags) => {
                let mut out = String::from("(flags");
                for flag in &flags.flags {
                    out.push_str(&format!(" \"{}\"", flag.name));
                }
                out.push(')');
                out
            }
            TypeDefKind::Tuple(tuple) => {
                let mut out = String::from("(tuple");
                for ty in &tuple.types {
                    out.push_str(&format!(" {}", self.value(ty)?));
                }
                out.push(')');
                out
            }
            TypeDefKind::Variant(variant) => {
                let mut out = String::from("(variant");
                for case in &variant.cases {
                    match &case.ty {
                        Some(ty) => {
                            out.push_str(&format!(" (case \"{}\" {})", case.name, self.value(ty)?))
                        }
                        None => out.push_str(&format!(" (case \"{}\")", case.name)),
                    }
                }
                out.push(')');
                out
            }
            TypeDefKind::Enum(en) => {
                let mut out = String::from("(enum");
                for case in &en.cases {
                    out.push_str(&format!(" \"{}\"", case.name));
                }
                out.push(')');
                out
            }
            TypeDefKind::Option(ty) => format!("(option {})", self.value(ty)?),
            TypeDefKind::Result(result) => {
                self.result_spelling(result.ok.as_ref(), result.err.as_ref())?
            }
            TypeDefKind::List(ty) => format!("(list {})", self.value(ty)?),
            TypeDefKind::Type(ty) => self.value(ty)?,
            TypeDefKind::Handle(wit_parser::Handle::Own(id)) => {
                format!("(own {})", self.name_of(*id)?)
            }
            TypeDefKind::Handle(wit_parser::Handle::Borrow(id)) => {
                format!("(borrow {})", self.name_of(*id)?)
            }
            TypeDefKind::Resource => {
                return Err(wasmtime::Error::msg("unreachable: resources emit above"));
            }
            other => {
                return Err(wasmtime::Error::msg(format!(
                    "WIT type `{}` is not available to a generated boundary",
                    other.as_str()
                )));
            }
        })
    }

    fn result_spelling(
        &mut self,
        ok: Option<&Type>,
        err: Option<&Type>,
    ) -> wasmtime::Result<String> {
        Ok(match (ok, err) {
            (Some(ok), Some(err)) => {
                format!("(result {} (error {}))", self.value(ok)?, self.value(err)?)
            }
            (Some(ok), None) => format!("(result {})", self.value(ok)?),
            (None, Some(err)) => format!("(result (error {}))", self.value(err)?),
            (None, None) => String::from("(result)"),
        })
    }

    /// The WAT spelling of a value type in a signature. Named types are
    /// references to an emitted declaration; anonymous types (`option<X>`,
    /// `result<_, _>`, `tuple<...>` written inline in a signature) render
    /// in place.
    fn value(&mut self, ty: &Type) -> wasmtime::Result<String> {
        Ok(match ty {
            Type::Bool => "bool".to_string(),
            Type::U8 => "u8".to_string(),
            Type::U16 => "u16".to_string(),
            Type::U32 => "u32".to_string(),
            Type::U64 => "u64".to_string(),
            Type::S8 => "s8".to_string(),
            Type::S16 => "s16".to_string(),
            Type::S32 => "s32".to_string(),
            Type::S64 => "s64".to_string(),
            Type::F32 => "float32".to_string(),
            Type::F64 => "float64".to_string(),
            Type::Char => "char".to_string(),
            Type::String => "string".to_string(),
            Type::ErrorContext => "error-context".to_string(),
            Type::Id(id) => {
                let anonymous = self.resolve.types[*id].name.is_none();
                if anonymous {
                    self.typedef_body(*id)?
                } else {
                    self.name_of(*id)?
                }
            }
        })
    }

    /// Emit one function: the instance export plus its alias for lowering.
    fn emit_func(
        &mut self,
        owner: InterfaceId,
        wit_name: &str,
        func: &wit_parser::Function,
    ) -> wasmtime::Result<Lower> {
        if func.kind.is_async() {
            return Err(wasmtime::Error::msg(format!(
                "WIT function `{wit_name}` is async; the generated boundary is synchronous"
            )));
        }
        // The functions map keys short names for freestanding functions and
        // the full `[method]resource.name` form for methods; the export name
        // is the key as-is.
        // The instance export name is the WIT key as-is; the `$fs` name is
        // `import_name`'s; the alias is a WAT identifier of the harness's own
        // making, so it keeps the bare method name and stays readable.
        let (export, import, alias) = match &func.kind {
            FunctionKind::Freestanding => (
                wit_name.to_string(),
                wit_name.to_string(),
                format!("{}-{wit_name}", self.prefix),
            ),
            FunctionKind::Method(resource) => {
                let resource_name = self.resolve.types[*resource]
                    .name
                    .clone()
                    .ok_or_else(|| wasmtime::Error::msg("WIT method on an unnamed resource"))?;
                if self.resolve.types[*resource].owner != wit_parser::TypeOwner::Interface(owner) {
                    return Err(wasmtime::Error::msg(format!(
                        "WIT method `{wit_name}` is on a foreign resource; the emitter handles own-interface methods"
                    )));
                }
                let bare = wit_name.rsplit('.').next().ok_or_else(|| {
                    wasmtime::Error::msg(format!("WIT method name `{wit_name}` has no `.`"))
                })?;
                (
                    wit_name.to_string(),
                    import_name(wit_name).to_string(),
                    format!("{}-{resource_name}-{bare}", self.prefix),
                )
            }
            _ => {
                return Err(wasmtime::Error::msg(format!(
                    "WIT function `{wit_name}` is a constructor or static; the emitter handles methods and freestanding functions"
                )));
            }
        };
        let mut params = String::new();
        // Newer `wit-parser` keeps the explicit `self: borrow<T>` parameter
        // on methods alongside `FunctionKind::Method`; the WAT spells it
        // once.
        let mut wit_params = func.params.as_slice();
        if let FunctionKind::Method(resource) = &func.kind {
            let resource_wat = self.name_of(*resource)?;
            params.push_str(&format!(" (param \"self\" (borrow {resource_wat}))"));
            if wit_params.first().is_some_and(|param| param.name == "self") {
                wit_params = &wit_params[1..];
            }
        }
        for param in wit_params {
            params.push_str(&format!(
                " (param \"{}\" {})",
                param.name,
                self.value(&param.ty)?
            ));
        }
        // The function wraps its return in one `(result ...)` level. An
        // anonymous `result<_, _>` renders in place through `value`, a named
        // one references its exported id; both are valid in that position.
        let result = match &func.result {
            Some(ty) => format!(" (result {})", self.value(ty)?),
            None => String::new(),
        };
        self.func_lines
            .push(format!("    (export \"{export}\" (func{params}{result}))"));
        self.aliases.push(format!(
            "  (alias export {} \"{export}\" (func {alias}))",
            self.instance
        ));
        Ok(Lower {
            export: import,
            core: format!("{alias}-l"),
            // Every generated lowering takes memory and realloc, whether its
            // signature visibly needs it or not: handle and record returns
            // validate only with memory present, and an unused option is
            // harmless.
            canon: format!("lower (func {alias}) (memory $memory) (realloc $realloc)"),
        })
    }
}
