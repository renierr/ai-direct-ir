# IR Options — Which IR Can Be a Final Binary Target?

Evaluated 2026-09-04.

| IR | Producer | Consumer / Runtime | Portable? | Spec size | AI-friendly? | Verdict |
|----|----------|--------------------|-----------|-----------|--------------|---------|
| LLVM IR | clang/rustc frontend | LLVM backend -> native | No (target-specific) | Huge, SSA, unstable text form | Low — easy to emit invalid SSA | Powerful but too complex for MVP |
| MLIR | dialects | lowers to LLVM / SPIR-V | No | Huge, extensible | Very low | For accelerators, not now |
| JVM bytecode | javac/kotlin | JVM (HotSpot, Graal) | Yes (JVM) | Medium, stack + GC-coupled | Medium | Tied to Java object model |
| .NET CIL | roslyn | CLR / NativeAOT | Yes (.NET) | Medium | Medium | Same lock-in issue |
| SPIR-V / eBPF | glsl/clang | GPU driver / kernel verifier | Domain-only | Small but narrow | Medium | Not general apps |
| **WASM (+ WASI)** | clang/rustc/Go/TinyGo, `wat2wasm` | browser, `wasmtime`, `wasmer`, `WasmEdge`, `WAMR` | **Yes** | **Small, ~200 opcodes, structured control flow, formal validation** | **High — WAT is regular S-expressions, validator catches errors** | **Pick for MVP** |

## Why WASM fits AI generation

1. Stack machine, no registers to allocate.
2. Structured control flow only: `block / loop / if / br / br_if`. No arbitrary goto.
3. Linear memory: one byte array, explicit `load/store`. No undefined behavior — validation fails fast.
4. Two forms, same semantics:
   - `WAT` (text, S-expressions) — what AI writes first
   - `.wasm` (binary, LEB128) — what AI emits later for size/speed
5. Optimizer exists: `wasm-opt -O3` (Binaryen), `wasm-tools`.
6. Capability-based system access via WASI imports, not syscalls — safer for AI output.

## What WASM does NOT give you

- No GC in MVP (MVP has `i32/i64/f32/f64` + memory; GC proposal / component-model types come later).
- No threads by default (threads proposal needs shared memory + atomics).
- No filesystem/network directly — must import from host (`wasi_snapshot_preview1.fd_write`, etc.).
