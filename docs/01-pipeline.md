# Compilation Pipeline — Traditional vs AI-Direct

## Traditional

```
source.languange
  -> lex / parse
  -> AST (language-specific, ambiguous, macros, name resolution)
  -> HIR (desugared)
  -> MIR (control-flow graph)
  -> LLVM IR / WASM (SSA or stack machine, validated)
  -> backend (regalloc, codegen, link)
  -> machine code
```

Frontend (source -> IR) exists to help humans. Backend (IR -> binary) exists to help machines.

## AI-Direct proposal

```
intent (natural language + tests)
  -> AI (acting as compiler frontend + backend-frontend)
  -> IR: WAT / WASM binary (validated by `wasm-tools validate`)
  -> optimizer: `wasm-opt -O3`
  -> execution: `wasmtime run` (JIT) or `wasmtime compile` (AOT) -> native
```

## What we gain / lose

Gain:
- No syntax ambiguities, no borrow checker fights, no build system.
- Output is mechanically checkable before running.
- One target runs everywhere (browser, server, edge, embedded).

Lose / must solve:
- Readability: WAT is verbose. Need WAT + tests + natural-language spec as source of truth.
- Debugging: need source maps / names section (`module $name`, `func $add`).
- High-level features: GC, closures, traits, async must be lowered by AI explicitly to linear memory + tables.
- System access: AI must learn WASI imports, not raw syscalls.

Open question: do we keep a thin human-readable spec (markdown + WAT) as the "source", and treat `.wasm` as build artifact?
