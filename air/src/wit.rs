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
//! The generated names extend the boundary ABI documented in `boundary.rs`:
//!
//! - `$fs-clock` — the `wasi:clocks/wall-clock` import, `datetime` only.
//! - `$fs-types` — the `wasi:filesystem/types` import.
//! - `$fs-pre` — the `wasi:filesystem/preopens` import.
//! - `$fs` — the core instance gathering every lowered filesystem function.
//!   An application links it like the WASI one: `(with "fs" (instance $fs))`
//!   and imports short names such as `"open-at"` and `"get-directories"`.

use std::collections::{HashMap, HashSet};

use wit_parser::{FunctionKind, InterfaceId, Resolve, Type, TypeDefKind, TypeId};

const IO_WIT: &str = include_str!("../wit/wasi-0.2.12/io.wit");
const CLOCKS_WIT: &str = include_str!("../wit/wasi-0.2.12/clocks.wit");
const FILESYSTEM_WIT: &str = include_str!("../wit/wasi-0.2.12/filesystem.wit");

/// One lowered filesystem function: the short name the application imports
/// from `"fs"`, the component-level function it lowers, and whether values
/// cross the boundary (lists, strings) so the lowering needs memory.
pub struct FsLower {
    pub export: String,
    pub func: String,
    pub needs_memory: bool,
}

/// The generated filesystem boundary: import text plus aliases, and the
/// lowering list `boundary.rs` turns into `$fs`.
pub struct Filesystem {
    pub wat: String,
    pub lowers: Vec<FsLower>,
}

/// Parse the vendored WASI WIT and emit the `wasi:filesystem` boundary.
///
/// Every type and signature below comes from that WIT. Only the instance
/// variable names (`$fs-types`, `$fs`, ...) are the harness's own ABI, exactly
/// like `$mem` and `$wasi`.
pub fn filesystem() -> wasmtime::Result<Filesystem> {
    let mut resolve = Resolve::new();
    for (path, contents) in [
        ("wasi-0.2.12/io.wit", IO_WIT),
        ("wasi-0.2.12/clocks.wit", CLOCKS_WIT),
        ("wasi-0.2.12/filesystem.wit", FILESYSTEM_WIT),
    ] {
        resolve.push_source(path, contents).map_err(|error| {
            wasmtime::Error::msg(format!("invalid vendored WIT `{path}`: {error:#}"))
        })?;
    }
    let wall_clock = interface(&resolve, "wasi:clocks/wall-clock@0.2.12")?;
    let types = interface(&resolve, "wasi:filesystem/types@0.2.12")?;
    let preopens = interface(&resolve, "wasi:filesystem/preopens@0.2.12")?;

    let mut out = String::new();
    out.push_str(
        "  ;; --- wasi:filesystem boundary, generated from WIT ----------------------\n\
         \x20 ;; `air/wit/wasi-0.2.12/filesystem.wit` is the source of truth.\n",
    );

    // `wasi:clocks/wall-clock`, `datetime` only: the one foreign type the
    // filesystem graph needs beyond `wasi:io`. Structural, like every other
    // generated declaration.
    let datetime = datetime_id(&resolve, wall_clock)?;
    let mut clock = Emitter::new(&resolve, "$fs-clock");
    clock.instance = "$fs-clock".to_string();
    clock.emit_type(wall_clock, "datetime", datetime)?;
    let clock_wat = clock.finish_instance("$fs-clock", "wasi:clocks/wall-clock@0.2.12");
    // Declare-then-export: an inline instance export only takes `eq`/`sub`.
    out.push_str(&clock_wat);
    out.push_str("  (alias export $fs-clock \"datetime\" (type $fs-datetime))\n\n");
    let mut foreign: HashMap<TypeId, String> = HashMap::new();
    foreign.insert(canon(&resolve, datetime), "$fs-datetime".to_string());
    // `wasi:io` resources arrive through the `@wasi` streams boundary, which
    // `filesystem` implies (see `Boundary::streams`). Refer to those names
    // directly, the way the hand-written boundary did.
    for (name, wat) in [
        ("input-stream", "$istream"),
        ("output-stream", "$ostream"),
        ("error", "$error"),
    ] {
        foreign.insert(canon(&resolve, io_type(&resolve, name)?), wat.to_string());
    }

    let mut types_emitter = Emitter::with_foreign(&resolve, "$fs", foreign);
    let types_lowers = types_emitter.emit_interface(types, "$fs-types")?;
    out.push_str(&types_emitter.finish_instance("$fs-types", "wasi:filesystem/types@0.2.12"));
    for alias in &types_emitter.aliases {
        out.push_str(alias);
        out.push('\n');
    }
    out.push('\n');

    // `preopens` re-exports the `descriptor` resource: alias the types
    // instance's export first, the way a hand-written import must. The new
    // emitter resolves nothing else from the types instance: every other
    // name it needs is local (or a `string`).
    out.push_str("  (alias export $fs-types \"descriptor\" (type $fs-pre-desc))\n");
    let mut pre_emitter = Emitter::new(&resolve, "$fs-pre");
    pre_emitter
        .names
        .insert(descriptor_id(&resolve, types)?, "$fs-pre-desc".to_string());
    let pre_lowers = pre_emitter.emit_interface(preopens, "$fs-pre")?;
    out.push_str(&pre_emitter.finish_instance("$fs-pre", "wasi:filesystem/preopens@0.2.12"));
    for alias in &pre_emitter.aliases {
        out.push_str(alias);
        out.push('\n');
    }

    let mut lowers = Vec::with_capacity(types_lowers.len() + pre_lowers.len());
    lowers.extend(types_lowers);
    lowers.extend(pre_lowers);
    // Two interfaces must not expose the same short name into `$fs`: the
    // application imports one flat `"fs"` namespace.
    let mut seen = HashSet::new();
    for lower in &lowers {
        if !seen.insert(lower.export.clone()) {
            return Err(wasmtime::Error::msg(format!(
                "WIT interface exports `{}` twice; cannot share one `$fs` namespace",
                lower.export
            )));
        }
    }
    Ok(Filesystem { wat: out, lowers })
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

fn datetime_id(resolve: &Resolve, wall_clock: InterfaceId) -> wasmtime::Result<TypeId> {
    resolve.interfaces[wall_clock]
        .types
        .get("datetime")
        .copied()
        .ok_or_else(|| wasmtime::Error::msg("wasi:clocks/wall-clock has no type `datetime`"))
}

fn descriptor_id(resolve: &Resolve, types: InterfaceId) -> wasmtime::Result<TypeId> {
    resolve.interfaces[types]
        .types
        .get("descriptor")
        .copied()
        .ok_or_else(|| wasmtime::Error::msg("wasi:filesystem/types has no type `descriptor`"))
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
    fn new(resolve: &'a Resolve, prefix: &str) -> Self {
        Self::with_foreign(resolve, prefix, HashMap::new())
    }

    fn with_foreign(resolve: &'a Resolve, prefix: &str, foreign: HashMap<TypeId, String>) -> Self {
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
    fn emit_interface(
        &mut self,
        id: InterfaceId,
        instance: &str,
    ) -> wasmtime::Result<Vec<FsLower>> {
        self.instance = instance.to_string();
        // Types first, in interface order: WIT declares before use.
        let type_names: Vec<(String, TypeId)> = self.resolve.interfaces[id]
            .types
            .iter()
            .map(|(name, type_id)| (name.clone(), *type_id))
            .collect();
        for (name, type_id) in type_names {
            self.emit_type(id, &name, type_id)?;
        }
        // Functions in interface order.
        let funcs: Vec<(String, wit_parser::Function)> = self.resolve.interfaces[id]
            .functions
            .iter()
            .map(|(name, func)| (name.clone(), func.clone()))
            .collect();
        let mut lowers = Vec::with_capacity(funcs.len());
        for (wit_name, func) in &funcs {
            lowers.push(self.emit_func(id, wit_name, func)?);
        }
        Ok(lowers)
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
    ) -> wasmtime::Result<FsLower> {
        if func.kind.is_async() {
            return Err(wasmtime::Error::msg(format!(
                "WIT function `{wit_name}` is async; the generated boundary is synchronous"
            )));
        }
        // The functions map keys short names for freestanding functions and
        // the full `[method]resource.name` form for methods; the export name
        // is the key as-is.
        let (export, short, alias) = match &func.kind {
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
                // Map keys (and `Function.name`) carry the full
                // `[method]resource.name` form; the `$fs` namespace wants the
                // short `name`.
                let short = wit_name
                    .rsplit('.')
                    .next()
                    .ok_or_else(|| {
                        wasmtime::Error::msg(format!("WIT method name `{wit_name}` has no `.`"))
                    })?
                    .to_string();
                (
                    wit_name.to_string(),
                    short.clone(),
                    format!("{}-{resource_name}-{short}", self.prefix),
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
        // A `result<_, _>` return arrives as a named `Result` typedef;
        // anything else is a plain value. Either way the function wraps it
        // in one `(result ...)` level.
        let result = match &func.result {
            Some(Type::Id(id))
                if matches!(&self.resolve.types[*id].kind, TypeDefKind::Result(_)) =>
            {
                let def = self.resolve.types[*id].clone();
                let TypeDefKind::Result(result) = &def.kind else {
                    unreachable!()
                };
                let inner = self.result_spelling(result.ok.as_ref(), result.err.as_ref())?;
                format!(" (result {inner})")
            }
            Some(ty) => format!(" (result {})", self.value(ty)?),
            None => String::new(),
        };
        self.func_lines
            .push(format!("    (export \"{export}\" (func{params}{result}))"));
        self.aliases.push(format!(
            "  (alias export {} \"{export}\" (func {alias}))",
            self.instance
        ));
        // Every filesystem lowering takes `(memory $memory) (realloc
        // $realloc)`, whether its signature visibly needs it or not: handle
        // and record returns validate only with memory present, and an
        // unused option is harmless. Verified by `air check` + `air run` on
        // the sha256sum example.
        Ok(FsLower {
            export: short,
            func: alias,
            needs_memory: true,
        })
    }
}
