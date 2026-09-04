# AI-Direct IR — Build Software Without a Programming Language

## Idea (2026-09-04)

Instead of: `AI -> source code (Rust/C/JS) -> compiler -> IR -> binary`

Do: `AI -> IR (WASM) directly -> runtime / native binary`

Rationale:
- Programming languages are optimized for humans to read, not for AI to write.
- IRs are smaller, more regular, verifiable. A module either validates or it doesn't.
- Skips parsing, AST, HIR/MIR lowering bugs. Fewer layers = fewer hallucinations.
- Existing optimizers (`wasm-opt`, AOT compilers) can still optimize the AI output.

Normal compiler for reference:
`source text -> lexer/parser -> AST -> HIR/MIR -> LLVM IR / WASM -> machine code (LLVM backend / wasmtime AOT) -> linker -> binary`

We replace the frontend with the AI.

## Why start with WASM?

See `docs/02-ir-options.md`. Summary: WASM is small (~200 opcodes), sandboxed, portable, has text form (WAT) that is LLM-friendly and binary form that is optimizable.

## How we run it

See `docs/03-wasm-runtime.md`. Summary: WASM needs a host. `wasmtime + WASI` for CLI/apps, browser for UI, embedded host for libraries.

## Status

- [x] Idea сформулирована / formulated
- [x] `hello.wat -> hello.wasm -> wasmtime run` proof of runtime (152 bytes, `wasmtime 48.0.1`, `wabt 1.0.41`, `binaryen 130`, `wasm-tools 1.257.1`; Python emitter byte-identical to `wat2wasm`)
- [x] AI-generated WAT for pure function
- [x] Interactive WAT app (`src/pi.wat` — stdin prompt, 0..1000 validation, spigot pi, bit-exact vs Chudnovsky at N=100/1000)
- [ ] AI-generated raw WASM binary + `wasm-opt`
- [x] Native exes via wasm2c (`native/hello-native`, `native/pi-native` — no runtime, N=1000 byte-identical, see `docs/13-wasm2c-native.md`)
- [ ] WASI Component Model target

See `docs/04-roadmap.md` and `docs/05-decisions.md`.
