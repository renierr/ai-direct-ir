# Stdlib question — Does WASM have libraries like other languages?

No. Core WASM has no stdlib. Only ~200 opcodes, linear memory, tables. No strings, no print, no files, no sockets, no allocator. Everything must be **imported from the host** or **bundled as another module**. Documented 2026-09-04.

## Where code comes from (3 sources)

### 1. Host imports (system API)
Module declares `(import "wasi_snapshot_preview1" "fd_write" ...)`, host (`wasmtime`) provides it. This is how our `hello.wasm` prints.

- WASI preview1: `fd_write`, `args_get`, `proc_exit`, `clock_time_get`, basic fs. Stable, what we use now.
- WASI 0.2 (Component Model): `wasi:cli/stdout`, `wasi:filesystem`, `wasi:http`, `wasi:keyvalue`, etc. Defined in WIT (WebAssembly Interface Types). Future target.

### 2. Bundled libraries (compiled to WASM)
Normal languages bring their stdlib by compiling it in:

- `wasi-sdk` (libc for C/C++), Rust `std` + `getrandom`, TinyGo runtime, AssemblyScript runtime, emscripten libs.
- If AI generates raw WASM, it gets none of this for free — must reimplement (bump allocator, memcpy, string ops) or link a prebuilt `stdlib.wasm`.

### 3. Component composition (reuse without recompiling)
WASI 0.2 Component Model + `wac` / `wasm-tools compose`: link multiple `.wasm` components via WIT interfaces. Closest thing to `pip/cargo` for raw WASM. Registries: `wapm.io` (wasmer), GHCR OCI artifacts, Bytecode Alliance component registry (emerging, fragmented).

## Consequence for AI-direct IR

AI must be explicit about what other languages hide:

- Memory management: no malloc — AI lays out linear memory itself (our hello uses fixed offsets 0/4/8/32) or emits a bump allocator.
- Strings/structs: just bytes + offsets + lengths (iovec pattern).
- System calls: only via WASI imports, capability-based.

## Practical path for this project

1. Now: depend only on `wasi_snapshot_preview1` + tiny hand-built helpers in WAT (our approach).
2. Next: build `stdlib.wat/wasm` — `print_str`, `alloc`, `memcpy`, `itoa` — once, reuse everywhere via copy-paste or `wasm-tools compose`.
3. Later: target WASI 0.2 / WIT components so AI emits `(import "wasi:cli/stdout" ...)` and links against shared components instead of reinventing libc.
