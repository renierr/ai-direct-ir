# Decisions (ADR-style log)

## 2026-09-04 — Use WASM as first IR
- Context: need small, validated, portable IR for AI to emit.
- Decision: WASM (+ WAT text form), not LLVM IR.
- Consequences: must learn WASI imports; defer GC/threads.

## 2026-09-04 — Use wasmtime + WASI preview1 for MVP runtime
- Context: need simplest `wat -> run` loop.
- Decision: `wasmtime`, `wat2wasm`, `wasm-tools`, `wasm-opt`.
- Consequences: pin versions in lab log; revisit WASI 0.2 components later.

## 2026-09-04 — Docs-first in empty folder
- Context: starting in `wasm-demo/` empty.
- Decision: keep findings in `README.md` + `docs/*.md`, code in `src/` (planned), log every experiment in `docs/06-lablog.md`.
- Consequences: no code without doc entry.
