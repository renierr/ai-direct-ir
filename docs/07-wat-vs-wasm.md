# WAT vs WASM — Text vs Binary, Same Module

Same semantics, two encodings. Our proof: `src/hello.wat` and `src/build_hello.py` produce byte-identical `hello.wasm` (152 bytes).

## WASM (`.wasm`) — what actually runs

- Binary format. Starts with magic `00 61 73 6D` (`\0asm`) + version `01 00 00 00`.
- Structured as sections: type(1), import(2), function(3), memory(5), export(7), code(10), data(11).
- Integers as LEB128, strings as length-prefixed bytes. Dense, fast to validate and JIT/AOT.
- What `wasmtime run`, browsers, `wasm-opt`, `wasm-tools validate` consume.
- Example: our `hello.wasm` is 152 bytes, imports `wasi_snapshot_preview1.fd_write`, exports `_start` + `memory`, one data segment `"hello from AI-direct IR\n"` at offset 8.

## WAT (`.wat`) — what humans (and AI in Phase 1) read/write

- Text form, S-expressions, ~1:1 mapping to binary. Comments, whitespace, names allowed.
- `wat2wasm src/hello.wat -o hello.wasm` assembles; `wasm2wat hello.wasm` / `wasm-tools print hello.wasm` disassembles.
- Our `src/hello.wat`:
  ```wat
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory 1)
  (data (i32.const 8) "hello from AI-direct IR\n")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 8))
    ...
    (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 32))
    drop)
  ```
- Names like `$fd_write` are erased in binary (unless names section kept for debugging).

## Analogy

`WAT : WASM = assembly text : machine code`. Same program, different encoding. Unlike C->assembly, the mapping is almost mechanical — no register allocation, no macros.

## Which do we generate?

| Phase | AI emits | Why |
|-------|----------|-----|
| Phase 0 (now) | Both (hand WAT + Python-built WASM) | Prove equivalence |
| Phase 1 (next) | **WAT** | LLM-friendly, readable, diffable, validator still catches errors. `wat2wasm` assembles. |
| Phase 2+ | **WASM binary directly** | Smaller/faster, no parser in loop. AI acts as assembler: emits sections + LEB128 (like `src/build_hello.py` does). `wasm-opt` then optimizes. |

Rule of thumb: **WAT for thinking, WASM for shipping.** Validator is the gate in both cases — invalid output fails fast before it ever runs.
