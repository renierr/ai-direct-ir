//! Generate the WASI 0.2 component boundary from a `;; @wasi ...` directive.
//!
//! Declaring the interfaces, lowering them into Core functions and exposing a
//! shared memory is the same ~55 lines in every component. It is also the most
//! error-prone part of one: a signature must reference the *exported* type id,
//! never the local type it was defined from, and getting that wrong rejects the
//! whole instance. It is mechanically derivable from the list of capabilities
//! an application uses, which is the argument that produced named data
//! segments.
//!
//! The generated names are the boundary's ABI, so an application can rely on
//! them:
//!
//! - `$memory` / `$realloc` — the shared linear memory and its bump allocator
//! - `$mem` — the core instance exporting `memory`, instantiate the app `(with
//!   "env" (instance $mem))`
//! - `$wasi` — the core instance of lowered imports, instantiate the app `(with
//!   "wasi" (instance $wasi))`
//! - `$fs` — the core instance of lowered `wasi:filesystem` imports, when the
//!   application asks for `filesystem` on its `;; @wasi` line. Instantiate the
//!   app `(with "fs" (instance $fs))` and import the names the WIT gives
//!   them: `"descriptor.open-at"`, `"get-directories"`.
//! - `$net` — the same for `wasi:sockets`, when the application asks for
//!   `sockets`: `(with "net" (instance $net))`, `"create-tcp-socket"`,
//!   `"tcp-socket.accept"`, `"pollable.block"`.
//!
//! Every type and signature in `$fs` and `$net` is generated from the vendored
//! WASI WIT (see `wit.rs`); only these instance names are harness ABI. The
//! `(import "fs" ...)` and `(import "net" ...)` lines are the request: the
//! capability declares those functions and no others.
//!
//! `$wasi` exports one Core function per requested capability: `get-stdin`,
//! `read`, `get-stdout`, `get-stderr`, `write`, `exit`, `exit-with-code`.
//!
//! `exit` takes a `result` discriminant — 0 or 1, nothing else — so it says
//! only whether the run failed. A program that wants a POSIX-style status asks
//! for `exit-with-code`, which takes a `u8`.

use std::collections::{BTreeMap, BTreeSet};

use wasmtime::Result;

/// What each core module of the expanded source imports, keyed by import
/// module name. The `"fs"` and `"net"` entries are what the WIT-derived
/// capabilities generate from.
pub type Imports = BTreeMap<String, BTreeSet<String>>;

/// The WASI release the generated boundary imports. One constant, because a
/// component that mixes versions imports two unrelated interfaces.
const VERSION: &str = "0.2.12";

const DEFAULT_PAGES: u32 = 1;
const DEFAULT_HEAP: u32 = 0x8000;

/// What an application asked for on its `;; @wasi` line.
#[derive(Debug)]
pub struct Boundary {
    pages: u32,
    heap: u32,
    stdin: bool,
    stdout: bool,
    stderr: bool,
    exit: bool,
    exit_with_code: bool,
    args: bool,
    filesystem: bool,
    sockets: bool,
}

impl Boundary {
    /// True when any capability needs `wasi:io/streams`. `exit` alone does not,
    /// so a component that only exits imports neither streams nor io/error.
    /// `filesystem` and `sockets` need the stream resources (`read-via-stream`
    /// returns an `input-stream`, `accept` returns both), so they imply the
    /// types even when no stdio capability asked for the methods.
    fn streams(&self) -> bool {
        self.stdin || self.stdout || self.stderr || self.filesystem || self.sockets
    }

    /// True when an output stream is in play; stdout and stderr share it.
    fn output(&self) -> bool {
        self.stdout || self.stderr
    }

    /// True when the boundary declares `input-stream.blocking-read`. `sockets`
    /// implies it: an accepted connection *is* an `input-stream`, and there is
    /// no other way to read one.
    fn read(&self) -> bool {
        self.stdin || self.sockets
    }

    /// The same for `output-stream.blocking-write-and-flush`.
    fn write(&self) -> bool {
        self.output() || self.sockets
    }
}

/// Parse the argument list of `;; @wasi <args>`.
///
/// Arguments are capability names (`stdin`, `stdout`, `stderr`, `exit`,
/// `exit-with-code`, `args`, `filesystem`, `sockets`) and settings
/// (`pages=<n>`, `heap=<addr>`).
/// Order does not matter and an unknown word is an error rather than a
/// silently ignored typo.
pub fn parse(args: &str) -> Result<Boundary> {
    let mut boundary = Boundary {
        pages: DEFAULT_PAGES,
        heap: DEFAULT_HEAP,
        stdin: false,
        stdout: false,
        stderr: false,
        exit: false,
        exit_with_code: false,
        args: false,
        filesystem: false,
        sockets: false,
    };
    for word in args.split_whitespace() {
        match word.split_once('=') {
            Some(("pages", value)) => boundary.pages = number(value, "pages")?,
            Some(("heap", value)) => boundary.heap = number(value, "heap")?,
            Some((name, _)) => {
                return Err(wasmtime::Error::msg(format!(
                    "unknown `@wasi` setting `{name}`; expected `pages=` or `heap=`"
                )));
            }
            None => match word {
                "stdin" => boundary.stdin = true,
                "stdout" => boundary.stdout = true,
                "stderr" => boundary.stderr = true,
                "exit" => boundary.exit = true,
                "exit-with-code" => boundary.exit_with_code = true,
                "args" => boundary.args = true,
                "filesystem" => boundary.filesystem = true,
                "sockets" => boundary.sockets = true,
                other => {
                    return Err(wasmtime::Error::msg(format!(
                        "unknown `@wasi` capability `{other}`; \
                         expected `stdin`, `stdout`, `stderr`, `exit`, \
                         `exit-with-code`, `args`, `filesystem` or `sockets`"
                    )));
                }
            },
        }
    }
    if boundary.pages == 0 {
        return Err(wasmtime::Error::msg("`@wasi pages=` must be at least 1"));
    }
    Ok(boundary)
}

/// Decimal, or hexadecimal with an `0x` prefix. Underscores separate digits.
fn number(value: &str, setting: &str) -> Result<u32> {
    let text = value.replace('_', "");
    let parsed = match text.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|_| wasmtime::Error::msg(format!("`@wasi {setting}={value}` is not a number")))
}

/// Emit the boundary. The result is ordinary WAT that an author could have
/// written; nothing here is privileged. The filesystem imports are derived
/// from the vendored WASI WIT on every emit, so this can fail.
///
/// `imports` are the names the expanded source imports from each module.
/// `wasi:filesystem` has 29 functions and `wasi:sockets` 39; an application
/// uses a handful, so the boundary declares the handful — the same rule the
/// capability list follows for `wasi:cli`.
pub fn emit(boundary: &Boundary, imports: &Imports) -> Result<String> {
    let mut out = String::new();
    out.push_str(
        "  ;; --- WASI 0.2 boundary, generated from `;; @wasi` --------------------\n\
         \x20 ;; Imports, shared memory and canonical ABI lowering. The application\n\
         \x20 ;; below sees ordinary Core functions on the `wasi` instance.\n",
    );
    if boundary.streams() {
        emit_streams(boundary, &mut out);
    }
    emit_cli(boundary, &mut out);
    let derived = [
        derive(
            boundary.filesystem,
            "filesystem",
            &crate::wit::FILESYSTEM,
            imports,
        )?,
        derive(boundary.sockets, "sockets", &crate::wit::SOCKETS, imports)?,
    ];
    for (_, generated) in derived.iter().flatten() {
        out.push_str(&generated.wat);
    }
    emit_memory(boundary, &mut out);
    emit_lowering(boundary, &derived, &mut out);
    Ok(out)
}

/// Generate one WIT-derived capability, if the directive asked for it. The
/// application's own imports from the capability's instance are the request,
/// so asking for the boundary and then importing nothing from it is a mistake
/// worth naming rather than an empty instance to puzzle over.
fn derive(
    asked: bool,
    word: &str,
    capability: &crate::wit::Capability,
    imports: &Imports,
) -> Result<Option<(&'static str, crate::wit::Generated)>> {
    if !asked {
        return Ok(None);
    }
    let instance = capability.instance;
    let used = imports.get(instance).cloned().unwrap_or_default();
    if used.is_empty() {
        return Err(wasmtime::Error::msg(format!(
            "`@wasi {word}` generates the `${instance}` instance, but no module \
             imports from `\"{instance}\"`; import the functions the application \
             calls, or drop `{word}`"
        )));
    }
    let generated = crate::wit::generate(capability, &used)?;
    Ok(Some((instance, generated)))
}

/// `wasi:io/error` and `wasi:io/streams`, carrying only the resources and
/// methods the requested capabilities use. `filesystem` and `sockets` name
/// both stream resources (`read-via-stream` returns an `input-stream`,
/// `accept` returns both), so they imply the types even when no stdio
/// capability asked for the methods.
fn emit_streams(boundary: &Boundary, out: &mut String) {
    // Resources the boundary itself declares.
    let input = boundary.read() || boundary.filesystem;
    let output = boundary.write() || boundary.filesystem;
    out.push_str(&format!(
        "  (import \"wasi:io/error@{VERSION}\" (instance $io-error\n\
         \x20   (export \"error\" (type (sub resource)))))\n\
         \x20 (alias export $io-error \"error\" (type $error))\n\n\
         \x20 (import \"wasi:io/streams@{VERSION}\" (instance $streams\n\
         \x20   (export \"error\" (type $ie (eq $error)))\n"
    ));
    if input {
        out.push_str("    (export \"input-stream\" (type $is (sub resource)))\n");
    }
    if output {
        out.push_str("    (export \"output-stream\" (type $os (sub resource)))\n");
    }
    // The signatures below must name `$sexp`, the *exported* id, not `$se`.
    out.push_str(
        "    (type $se (variant (case \"last-operation-failed\" (own $ie)) (case \"closed\")))\n\
         \x20   (export \"stream-error\" (type $sexp (eq $se)))\n",
    );
    if boundary.read() {
        out.push_str(
            "    (export \"[method]input-stream.blocking-read\"\n\
             \x20     (func (param \"self\" (borrow $is)) (param \"len\" u64)\n\
             \x20           (result (result (list u8) (error $sexp)))))\n",
        );
    }
    if boundary.write() {
        out.push_str(
            "    (export \"[method]output-stream.blocking-write-and-flush\"\n\
             \x20     (func (param \"self\" (borrow $os)) (param \"contents\" (list u8))\n\
             \x20           (result (result (error $sexp)))))\n",
        );
    }
    out.push_str("    ))\n");
    // A resource is aliased whenever it is declared: a generated capability
    // refers to `$istream` / `$ostream` by name.
    if input {
        out.push_str("  (alias export $streams \"input-stream\" (type $istream))\n");
    }
    if boundary.read() {
        out.push_str(
            "  (alias export $streams \"[method]input-stream.blocking-read\" (func $read))\n",
        );
    }
    if output {
        out.push_str("  (alias export $streams \"output-stream\" (type $ostream))\n");
    }
    if boundary.write() {
        out.push_str(
            "  (alias export $streams \"[method]output-stream.blocking-write-and-flush\" (func $write))\n",
        );
    }
    out.push('\n');
}

/// One `wasi:cli` import per requested capability.
fn emit_cli(boundary: &Boundary, out: &mut String) {
    if boundary.stdin {
        out.push_str(&format!(
            "  (import \"wasi:cli/stdin@{VERSION}\" (instance $stdin\n\
             \x20   (export \"input-stream\" (type (eq $istream)))\n\
             \x20   (export \"get-stdin\" (func (result (own $istream))))))\n\
             \x20 (alias export $stdin \"get-stdin\" (func $get-stdin))\n\n"
        ));
    }
    if boundary.stdout {
        out.push_str(&format!(
            "  (import \"wasi:cli/stdout@{VERSION}\" (instance $stdout\n\
             \x20   (export \"output-stream\" (type (eq $ostream)))\n\
             \x20   (export \"get-stdout\" (func (result (own $ostream))))))\n\
             \x20 (alias export $stdout \"get-stdout\" (func $get-stdout))\n\n"
        ));
    }
    if boundary.stderr {
        out.push_str(&format!(
            "  (import \"wasi:cli/stderr@{VERSION}\" (instance $stderr\n\
             \x20   (export \"output-stream\" (type (eq $ostream)))\n\
             \x20   (export \"get-stderr\" (func (result (own $ostream))))))\n\
             \x20 (alias export $stderr \"get-stderr\" (func $get-stderr))\n\n"
        ));
    }
    if boundary.args {
        out.push_str(&format!(
            "  (import \"wasi:cli/environment@{VERSION}\" (instance $env\n\
             \x20   (export \"get-arguments\" (func (result (list string))))))\n\
             \x20 (alias export $env \"get-arguments\" (func $get-args))\n\n"
        ));
    }
    if boundary.exit || boundary.exit_with_code {
        out.push_str(&format!(
            "  (import \"wasi:cli/exit@{VERSION}\" (instance $exit-i\n"
        ));
        if boundary.exit {
            out.push_str("    (export \"exit\" (func (param \"status\" (result))))\n");
        }
        if boundary.exit_with_code {
            out.push_str("    (export \"exit-with-code\" (func (param \"status-code\" u8)))\n");
        }
        out.push_str("    ))\n");
        if boundary.exit {
            out.push_str("  (alias export $exit-i \"exit\" (func $exit-fn))\n");
        }
        if boundary.exit_with_code {
            out.push_str("  (alias export $exit-i \"exit-with-code\" (func $exit-code-fn))\n");
        }
        out.push('\n');
    }
}

/// The shared memory module. Lowering an import needs the memory and the
/// application module needs the lowered imports, so the memory lives in a
/// module of its own to break that cycle.
fn emit_memory(boundary: &Boundary, out: &mut String) {
    out.push_str(&format!(
        "  (core module $mem-mod\n\
         \x20   (memory (export \"memory\") {pages})\n\
         \x20   (global $bump (mut i32) (i32.const {heap:#x}))\n\
         \x20   ;; The canonical ABI allocates host-produced values here. A bump\n\
         \x20   ;; allocator is enough: a component built this way never frees.\n\
         \x20   (func (export \"cabi_realloc\")\n\
         \x20     (param $old i32) (param $old_size i32) (param $align i32) (param $new i32)\n\
         \x20     (result i32)\n\
         \x20     (local $ptr i32)\n\
         \x20     (global.set $bump\n\
         \x20       (i32.and (i32.add (global.get $bump) (i32.sub (local.get $align) (i32.const 1)))\n\
         \x20                (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))\n\
         \x20     (local.set $ptr (global.get $bump))\n\
         \x20     (global.set $bump (i32.add (global.get $bump) (local.get $new)))\n\
         \x20     (local.get $ptr)))\n\
         \x20 (core instance $mem (instantiate $mem-mod))\n\
         \x20 (alias core export $mem \"memory\" (core memory $memory))\n\
         \x20 (alias core export $mem \"cabi_realloc\" (core func $realloc))\n\n",
        pages = boundary.pages,
        heap = boundary.heap,
    ));
}

/// Lower each import into a Core function and gather them into `$wasi` — and,
/// for each WIT-derived capability, into its own instance. Functions that move
/// lists across the boundary need memory and realloc; handle-only and empty
/// ones do not.
fn emit_lowering(
    boundary: &Boundary,
    derived: &[Option<(&str, crate::wit::Generated)>],
    out: &mut String,
) {
    let mut lowered: Vec<(&str, &str, bool)> = Vec::new();
    if boundary.stdin {
        lowered.push(("get-stdin", "$get-stdin", false));
    }
    if boundary.read() {
        lowered.push(("read", "$read", true));
    }
    if boundary.stdout {
        lowered.push(("get-stdout", "$get-stdout", false));
    }
    if boundary.stderr {
        lowered.push(("get-stderr", "$get-stderr", false));
    }
    if boundary.write() {
        lowered.push(("write", "$write", true));
    }
    if boundary.args {
        // A list of strings is allocated into the guest, so this one needs both.
        lowered.push(("get-arguments", "$get-args", true));
    }
    if boundary.exit {
        lowered.push(("exit", "$exit-fn", false));
    }
    if boundary.exit_with_code {
        lowered.push(("exit-with-code", "$exit-code-fn", false));
    }
    for (name, func, needs_memory) in &lowered {
        let extra = if *needs_memory {
            " (memory $memory) (realloc $realloc)"
        } else {
            ""
        };
        out.push_str(&format!(
            "  (core func ${name}-l (canon lower (func {func}){extra}))\n"
        ));
    }
    out.push_str("  (core instance $wasi\n");
    for (name, _, _) in &lowered {
        out.push_str(&format!("    (export \"{name}\" (func ${name}-l))\n"));
    }
    out.push_str("  )\n");
    for (instance, generated) in derived.iter().flatten() {
        // Every generated lowering takes memory and realloc, whether its
        // signature visibly needs it or not: handle and record returns
        // validate only with memory present, and an unused option is
        // harmless. Verified by `air check` + `air run` on sha256sum.
        for lower in &generated.lowers {
            out.push_str(&format!(
                "  (core func {}-l (canon lower (func {}) \
                 (memory $memory) (realloc $realloc)))\n",
                lower.func, lower.func
            ));
        }
        out.push_str(&format!("  (core instance ${instance}\n"));
        for lower in &generated.lowers {
            out.push_str(&format!(
                "    (export \"{}\" (func {}-l))\n",
                lower.export, lower.func
            ));
        }
        out.push_str("  )\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boundary of `args`, for a program that imports no capability.
    fn wat(args: &str) -> String {
        emit(&parse(args).unwrap(), &Imports::new()).unwrap()
    }

    fn wat_fs(args: &str, names: &[&str]) -> String {
        try_wat(args, "fs", names).unwrap()
    }

    fn wat_net(args: &str, names: &[&str]) -> String {
        try_wat(args, "net", names).unwrap()
    }

    /// The boundary of `args` for a program importing `names` from `module`.
    fn try_wat(args: &str, module: &str, names: &[&str]) -> Result<String> {
        let mut imports = Imports::new();
        imports.insert(
            module.to_string(),
            names.iter().map(|name| name.to_string()).collect(),
        );
        emit(&parse(args).unwrap(), &imports)
    }

    #[test]
    fn defaults_are_one_page_and_a_high_heap() {
        let boundary = parse("stdout").unwrap();
        assert_eq!(boundary.pages, DEFAULT_PAGES);
        assert_eq!(boundary.heap, DEFAULT_HEAP);
        assert!(boundary.stdout && !boundary.stdin);
    }

    #[test]
    fn settings_accept_decimal_and_hex() {
        let boundary = parse("stdin stdout pages=2 heap=0x1_0000").unwrap();
        assert_eq!(boundary.pages, 2);
        assert_eq!(boundary.heap, 0x10000);
    }

    #[test]
    fn a_typo_is_an_error_not_a_silent_omission() {
        let message = parse("stdout stdinn").unwrap_err().to_string();
        assert!(message.contains("stdinn"), "{message}");
        assert!(parse("stdout pages=x").is_err());
        assert!(parse("stdout size=2").is_err());
        assert!(parse("stdout pages=0").is_err());
    }

    /// `exit` takes a `result`, so only 0 and 1 are representable. Passing an
    /// exit code to it traps on an unexpected discriminant, which is why a
    /// program that wants a status code asks for `exit-with-code` instead.
    #[test]
    fn exit_and_exit_with_code_are_separate_capabilities() {
        let only_exit = wat("exit");
        assert!(only_exit.contains("(export \"exit\" (func (param \"status\" (result))))"));
        assert!(!only_exit.contains("exit-with-code"), "{only_exit}");

        let both = wat("exit exit-with-code");
        assert!(both.contains("(export \"exit-with-code\" (func (param \"status-code\" u8)))"));
        // One interface instance carries both functions.
        assert_eq!(both.matches("wasi:cli/exit@").count(), 1);
        assert!(both.contains("$exit-l") && both.contains("$exit-with-code-l"));

        let only_code = wat("exit-with-code");
        assert!(only_code.contains("exit-with-code"));
        assert!(!only_code.contains("(export \"exit\" (func"), "{only_code}");
    }

    #[test]
    fn exit_alone_needs_no_stream_interfaces() {
        let text = wat("exit");
        assert!(!text.contains("wasi:io/streams"), "{text}");
        assert!(!text.contains("wasi:io/error"), "{text}");
        assert!(text.contains("wasi:cli/exit"));
    }

    #[test]
    fn stdout_and_stderr_share_one_output_stream() {
        let text = wat("stdout stderr");
        assert_eq!(
            text.matches("(export \"output-stream\" (type $os").count(),
            1
        );
        assert_eq!(text.matches("(core func $write-l").count(), 1);
        assert!(text.contains("$get-stdout-l") && text.contains("$get-stderr-l"));
    }

    /// The bug that rejected a whole instance once: a signature must reference
    /// the exported id, never the local type it was defined from.
    #[test]
    fn signatures_reference_the_exported_stream_error_id() {
        let text = wat("stdin stdout");
        assert!(text.contains("(error $sexp)"), "{text}");
        assert!(!text.contains("(error $se)"), "{text}");
    }

    /// `filesystem` derives the whole `wasi:filesystem` boundary from the
    /// vendored WIT: the 37-case `error-code` enum and every method signature
    /// arrive without transcription, and `$fs` carries the lowered functions.
    #[test]
    fn filesystem_comes_from_wit_not_transcription() {
        let text = wat_fs(
            "stdout filesystem",
            &["descriptor.open-at", "get-directories"],
        );
        assert!(text.contains("wasi:filesystem/types@0.2.12"), "{text}");
        assert!(text.contains("wasi:filesystem/preopens@0.2.12"), "{text}");
        // The enum the hand-written boundary transcribed case by case.
        assert!(text.contains("\"cross-device\""), "{text}");
        assert!(text.contains("\"not-permitted\""), "{text}");
        // Methods lower into `$fs` qualified by their resource, freestanding
        // functions under their own name.
        assert!(
            text.contains("(export \"descriptor.open-at\" (func $"),
            "{text}"
        );
        assert!(
            text.contains("(export \"get-directories\" (func $"),
            "{text}"
        );
        assert!(text.contains("(core instance $fs"), "{text}");
    }

    #[test]
    fn filesystem_without_stdio_still_imports_stream_types() {
        // `read-via-stream` returns an `input-stream`: the resource types must
        // exist even when no stdio capability asked for the methods.
        let text = wat_fs("filesystem", &["descriptor.read-via-stream"]);
        assert!(text.contains("wasi:io/streams@"), "{text}");
        assert!(text.contains("wasi:filesystem/types@"), "{text}");
        assert!(!text.contains("wasi:cli/stdout@"), "{text}");
    }

    /// The point of deriving the boundary: an application that names three
    /// functions carries three, not the WIT's whole catalogue.
    #[test]
    fn filesystem_declares_only_the_imported_functions() {
        let text = wat_fs(
            "stdin stdout filesystem",
            &[
                "get-directories",
                "descriptor.open-at",
                "descriptor.read-via-stream",
            ],
        );
        for wanted in [
            "descriptor.open-at",
            "descriptor.read-via-stream",
            "get-directories",
        ] {
            assert!(
                text.contains(&format!("(export \"{wanted}\" (func $")),
                "{text}"
            );
        }
        // Unasked-for methods and the types only they reach stay out.
        for unwanted in [
            "write-via-stream",
            "set-times",
            "metadata-hash",
            "sync-data",
        ] {
            assert!(!text.contains(unwanted), "{unwanted} leaked into:\n{text}");
        }
        assert!(!text.contains("descriptor-stat"), "{text}");
        // `descriptor-stat` is the only thing that reaches `datetime`, so the
        // wall-clock import goes with it.
        assert!(!text.contains("wasi:clocks/wall-clock"), "{text}");
    }

    /// Naming a function the WIT does not have is a typo, not a link failure
    /// three steps later.
    #[test]
    fn an_unknown_fs_import_is_an_error() {
        let message = try_wat("filesystem", "fs", &["descriptor.open-att"])
            .unwrap_err()
            .to_string();
        assert!(message.contains("descriptor.open-att"), "{message}");

        // So is asking for the boundary and then importing nothing from it.
        let message = emit(&parse("filesystem").unwrap(), &Imports::new())
            .unwrap_err()
            .to_string();
        assert!(message.contains("no module"), "{message}");
        let message = emit(&parse("sockets").unwrap(), &Imports::new())
            .unwrap_err()
            .to_string();
        assert!(message.contains("$net"), "{message}");
    }

    /// `preopens` re-exports `descriptor`, so the types instance must export it
    /// even when the application calls no descriptor method.
    #[test]
    fn preopens_alone_still_exports_the_descriptor_resource() {
        let text = wat_fs("filesystem", &["get-directories"]);
        assert!(
            text.contains("(export \"descriptor\" (type $fs-types-descriptor-t1 (sub resource)))"),
            "{text}"
        );
        assert!(
            text.contains("(alias export $fs-types \"descriptor\" (type $fs-pre-descriptor))"),
            "{text}"
        );
        assert!(!text.contains("(export \"descriptor.open-at\""), "{text}");
    }

    /// The names a TCP listener needs. `wasi:sockets` spreads them over four
    /// interfaces plus `wasi:io/poll`, and the emitter threads the shared
    /// `network`, `error-code` and `pollable` declarations between them
    /// instead of repeating each graph.
    const LISTEN: &[&str] = &[
        "instance-network",
        "create-tcp-socket",
        "tcp-socket.start-bind",
        "tcp-socket.finish-bind",
        "tcp-socket.start-listen",
        "tcp-socket.finish-listen",
        "tcp-socket.accept",
        "tcp-socket.subscribe",
        "pollable.block",
    ];

    #[test]
    fn sockets_come_from_wit_across_five_interfaces() {
        let text = wat_net("sockets", LISTEN);
        for wit in [
            "wasi:io/poll@0.2.12",
            "wasi:sockets/network@0.2.12",
            "wasi:sockets/instance-network@0.2.12",
            "wasi:sockets/tcp@0.2.12",
            "wasi:sockets/tcp-create-socket@0.2.12",
        ] {
            assert!(text.contains(wit), "missing {wit} in:\n{text}");
        }
        // `error-code` is declared once, by `network`, and aliased onward.
        assert_eq!(text.matches("\"address-in-use\"").count(), 1, "{text}");
        assert!(
            text.contains("(alias export $net-network \"error-code\" (type $net-tcp-error-code))"),
            "{text}"
        );
        // `pollable` likewise, from `wasi:io/poll`.
        assert!(
            text.contains("(alias export $net-poll \"pollable\" (type $net-tcp-pollable))"),
            "{text}"
        );
        assert!(text.contains("(core instance $net"), "{text}");
    }

    /// Sockets read and write streams with no stdio in sight: an accepted
    /// connection *is* an `input-stream`, so the boundary must carry the
    /// methods even when the directive names no stdio capability.
    #[test]
    fn sockets_imply_the_stream_methods_without_stdio() {
        let text = wat_net("sockets", LISTEN);
        assert!(
            text.contains("[method]input-stream.blocking-read"),
            "{text}"
        );
        assert!(
            text.contains("[method]output-stream.blocking-write-and-flush"),
            "{text}"
        );
        assert!(text.contains("(export \"read\" (func $read-l))"), "{text}");
        assert!(
            text.contains("(export \"write\" (func $write-l))"),
            "{text}"
        );
        assert!(!text.contains("wasi:cli/stdin@"), "{text}");
        assert!(!text.contains("wasi:cli/stdout@"), "{text}");
    }

    /// UDP is 20 of the 39 functions and its own resource graph. A TCP
    /// listener carries none of it -- nor the interfaces it would come from.
    #[test]
    fn a_tcp_listener_carries_no_udp() {
        let text = wat_net("sockets", LISTEN);
        for unwanted in [
            "wasi:sockets/udp",
            "wasi:sockets/ip-name-lookup",
            "wasi:clocks/monotonic-clock",
            "keep-alive",
            "incoming-datagram",
        ] {
            assert!(!text.contains(unwanted), "{unwanted} leaked into:\n{text}");
        }
    }

    /// Both capabilities at once: two instances, two namespaces, one memory.
    #[test]
    fn filesystem_and_sockets_share_one_boundary() {
        let mut imports = Imports::new();
        imports.insert(
            "fs".to_string(),
            ["get-directories".to_string()].into_iter().collect(),
        );
        imports.insert(
            "net".to_string(),
            ["instance-network".to_string()].into_iter().collect(),
        );
        let text = emit(&parse("stdout filesystem sockets").unwrap(), &imports).unwrap();
        assert!(text.contains("(core instance $fs"), "{text}");
        assert!(text.contains("(core instance $net"), "{text}");
        assert_eq!(text.matches("(core module $mem-mod").count(), 1, "{text}");
    }
}
