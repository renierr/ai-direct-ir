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
//!
//! `$wasi` exports one Core function per requested capability: `get-stdin`,
//! `read`, `get-stdout`, `get-stderr`, `write`, `exit`.

use wasmtime::Result;

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
}

impl Boundary {
    /// True when any capability needs `wasi:io/streams`. `exit` alone does not,
    /// so a component that only exits imports neither streams nor io/error.
    fn streams(&self) -> bool {
        self.stdin || self.stdout || self.stderr
    }

    /// True when an output stream is in play; stdout and stderr share it.
    fn output(&self) -> bool {
        self.stdout || self.stderr
    }
}

/// Parse the argument list of `;; @wasi <args>`.
///
/// Arguments are capability names (`stdin`, `stdout`, `stderr`, `exit`) and
/// settings (`pages=<n>`, `heap=<addr>`). Order does not matter and an unknown
/// word is an error rather than a silently ignored typo.
pub fn parse(args: &str) -> Result<Boundary> {
    let mut boundary = Boundary {
        pages: DEFAULT_PAGES,
        heap: DEFAULT_HEAP,
        stdin: false,
        stdout: false,
        stderr: false,
        exit: false,
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
                other => {
                    return Err(wasmtime::Error::msg(format!(
                        "unknown `@wasi` capability `{other}`; \
                         expected `stdin`, `stdout`, `stderr` or `exit`"
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
/// written; nothing here is privileged.
pub fn emit(boundary: &Boundary) -> String {
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
    emit_memory(boundary, &mut out);
    emit_lowering(boundary, &mut out);
    out
}

/// `wasi:io/error` and `wasi:io/streams`, carrying only the resources and
/// methods the requested capabilities use.
fn emit_streams(boundary: &Boundary, out: &mut String) {
    out.push_str(&format!(
        "  (import \"wasi:io/error@{VERSION}\" (instance $io-error\n\
         \x20   (export \"error\" (type (sub resource)))))\n\
         \x20 (alias export $io-error \"error\" (type $error))\n\n\
         \x20 (import \"wasi:io/streams@{VERSION}\" (instance $streams\n\
         \x20   (export \"error\" (type $ie (eq $error)))\n"
    ));
    if boundary.stdin {
        out.push_str("    (export \"input-stream\" (type $is (sub resource)))\n");
    }
    if boundary.output() {
        out.push_str("    (export \"output-stream\" (type $os (sub resource)))\n");
    }
    // The signatures below must name `$sexp`, the *exported* id, not `$se`.
    out.push_str(
        "    (type $se (variant (case \"last-operation-failed\" (own $ie)) (case \"closed\")))\n\
         \x20   (export \"stream-error\" (type $sexp (eq $se)))\n",
    );
    if boundary.stdin {
        out.push_str(
            "    (export \"[method]input-stream.blocking-read\"\n\
             \x20     (func (param \"self\" (borrow $is)) (param \"len\" u64)\n\
             \x20           (result (result (list u8) (error $sexp)))))\n",
        );
    }
    if boundary.output() {
        out.push_str(
            "    (export \"[method]output-stream.blocking-write-and-flush\"\n\
             \x20     (func (param \"self\" (borrow $os)) (param \"contents\" (list u8))\n\
             \x20           (result (result (error $sexp)))))\n",
        );
    }
    out.push_str("    ))\n");
    if boundary.stdin {
        out.push_str(
            "  (alias export $streams \"input-stream\" (type $istream))\n\
             \x20 (alias export $streams \"[method]input-stream.blocking-read\" (func $read))\n",
        );
    }
    if boundary.output() {
        out.push_str(
            "  (alias export $streams \"output-stream\" (type $ostream))\n\
             \x20 (alias export $streams \"[method]output-stream.blocking-write-and-flush\" (func $write))\n",
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
    if boundary.exit {
        out.push_str(&format!(
            "  (import \"wasi:cli/exit@{VERSION}\" (instance $exit-i\n\
             \x20   (export \"exit\" (func (param \"status\" (result))))))\n\
             \x20 (alias export $exit-i \"exit\" (func $exit-fn))\n\n"
        ));
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

/// Lower each import into a Core function and gather them into `$wasi`.
/// Functions that move lists across the boundary need memory and realloc;
/// handle-only and empty ones do not.
fn emit_lowering(boundary: &Boundary, out: &mut String) {
    let mut lowered: Vec<(&str, &str, bool)> = Vec::new();
    if boundary.stdin {
        lowered.push(("get-stdin", "$get-stdin", false));
        lowered.push(("read", "$read", true));
    }
    if boundary.stdout {
        lowered.push(("get-stdout", "$get-stdout", false));
    }
    if boundary.stderr {
        lowered.push(("get-stderr", "$get-stderr", false));
    }
    if boundary.output() {
        lowered.push(("write", "$write", true));
    }
    if boundary.exit {
        lowered.push(("exit", "$exit-fn", false));
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
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn exit_alone_needs_no_stream_interfaces() {
        let text = emit(&parse("exit").unwrap());
        assert!(!text.contains("wasi:io/streams"), "{text}");
        assert!(!text.contains("wasi:io/error"), "{text}");
        assert!(text.contains("wasi:cli/exit"));
    }

    #[test]
    fn stdout_and_stderr_share_one_output_stream() {
        let text = emit(&parse("stdout stderr").unwrap());
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
        let text = emit(&parse("stdin stdout").unwrap());
        assert!(text.contains("(error $sexp)"), "{text}");
        assert!(!text.contains("(error $se)"), "{text}");
    }
}
