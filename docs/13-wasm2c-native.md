# wasm2c → Native Exe: What, Why, and Is It Safe?

Experiment done 2026-09-04. Question: real standalone binary from our `.wasm`, and does it stay memory-safe?

## What wasm2c is and what it needed

`wasm2c` (ships with wabt, `/usr/bin/wasm2c`) translates a `.wasm` module into portable C99 (`gen/hello.c` ~30 KB + `gen/hello.h`). You then compile it with any C compiler. Needed pieces, all already on this machine:

- `wasm2c` itself + `wasm-rt` runtime (`/usr/include/wasm-rt.h`, `wasm-rt-impl.c`, `wasm-rt-mem-impl.c` — all in Arch `wabt` package, no installs).
- Handwritten host layer for the module's imports — for us just 3 functions (`native/wasi_shim.c`, ~150 lines: `fd_write`→`write`, `fd_read`→`read`, `proc_exit`→`exit`, real WASI errno values).
- A `main()` that inits, instantiates, calls `_start` (`native/main_hello.c`, `native/main_pi.c`, ~20 lines each). Reproducible via `native/build.sh`.

Why this route instead of shipping wasmtime: zero-dependency output. Result is ELF x86-64 linked only against system libc (`ldd` shows `libc.so.6` and nothing else) — 23 KB exes, no runtime to install, auditable C in between.

## Verified results

- `hello-native` → `hello from AI-direct IR`; `pi-native` correct for N=0/1/10/25/100/1000.
- **N=1000 output byte-identical** between `wasmtime run ../src/pi.wasm` and `./pi-native` (1003 bytes each).
- Invalid inputs (`abc`, `1001`) → exit 1 in both. Same behavior, no runtime.

## Safety analysis (memory-bug wise) — honest version

**What stays safe:** bugs *in the WASM module* cannot escape. Verified in the generated code: every load/store goes through `RANGE_CHECK`/`DEFINE_LOAD` macros that trap (`wasm_rt_trap`) on OOB instead of corrupting memory; arithmetic keeps WASM wrap semantics; indirect calls check table bounds + signatures. A malicious/buggy `.wat` can at worst trap or return an error — same guarantee as under wasmtime.

**What moves (the new TCB):** the sandbox is gone, replaced by three things you must trust instead:

1. **`wasi_shim.c` (ours, plain C)** — the critical layer. It receives raw `(offset,len)` pairs from the module and dereferences them as host pointers. Every pair is validated overflow-safe (`(u64)off+(u64)len <= mem->size`) before use; violation returns `EFAULT` instead of touching memory. Rule: keep this file small, boring, and fully range-checked — it is the entire containment boundary now. Built with `-fstack-protector-strong -D_FORTIFY_SOURCE=2`.
2. **wasm2c codegen + `wasm-rt`** — mature (wabt project) but a bigger, less fuzzed TCB than wasmtime's engine. A codegen bug here is a native-memory bug.
3. **No speculative-sandbox mitigations** — wasmtime hardens against Spectre-style sandbox escapes; native C doesn't need to (there's no sandbox left to escape), but the flip side is: only run *trusted* modules this way. Threat model: wasm2c = distribution format for your own code; wasmtime = execution engine for untrusted code.

Also: deep WASM recursion becomes deep C-stack recursion (no explicit stack-limit instrumentation by default — `WASM_RT_MAX_CALL_STACK_DEPTH` exists for host-call depth, tunable in `wasm-rt.h`).

## Guideline for this project

- Prefer `wasmtime run/serve` (sandboxed) during dev and for anything ingesting untrusted input/code.
- Use wasm2c for release artifacts of *our own verified* modules (validator + differential tests like the N=1000 cross-check must pass first).
- Keep every module's import surface minimal (pi: 3 funcs) — each import is handwritten C and audit surface. Never `wasm2c` a module whose imports you haven't read.
