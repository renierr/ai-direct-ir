# Roadmap — Step by Step to AI-Direct Binary

## Phase 0 — Prove runtime (manual, no AI yet)
- [ ] Install: `wat2wasm` (wabt), `wasm-tools`, `wasm-opt` (binaryen), `wasmtime`
- [ ] Hand-write `src/hello.wat` using `wasi_snapshot_preview1.fd_write` -> `hello world`
- [ ] `wat2wasm + wasm-tools validate + wasmtime run` passes
- [ ] Document exact versions + commands in `docs/06-lablog.md`

Success: we can run any WAT without a programming language toolchain.

## Phase 1 — AI writes WAT (text IR)
- [ ] Prompt AI with natural language spec + expected I/O -> WAT only
- [ ] Pure function first: `add(i32,i32)->i32`, then `fib`, then string in linear memory
- [ ] Validate mechanically, no human fixing of logic
- [ ] Keep `(module $name ... (func $name ...))` + names section for debuggability

Success: untrusted AI text still caught by validator before execution.

## Phase 2 — AI emits binary WASM
- [ ] AI uses a builder lib (e.g. python `wasm` / JS `binaryen`) or raw LEB128, not hand text
- [ ] Compare size/speed: hand WAT vs `wasm-opt -O3` output
- [ ] Add tests: assert exported function results via `wasmtime` + python host

Success: smaller/faster artifact, still validated.

## Phase 3 — System apps via WASI
- [ ] Target `fd_write`, `args_get`, `proc_exit`, files, clocks
- [ ] Build CLI: `echo`, `cat`-like tool fully AI-generated
- [ ] Move to WASI 0.2 Component Model (`wasi:cli/run`) when preview1 works

Success: real usable binary with no source language.

## Phase 4 — Optimize + compose
- [ ] `wasm-opt`, AOT `wasmtime compile`, measure cold start / size
- [ ] Compose components: `wac` / `wasm-tools compose`
- [ ] ADR: when to add GC proposal, threads, `wasm-gc` for high-level languages

Non-goals for now: custom LLVM backend, kernel/GPU targets, self-hosting AI compiler.
