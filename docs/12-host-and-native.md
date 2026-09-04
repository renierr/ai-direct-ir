# HOST, Portability, and Native Apps

Questions (2026-09-04): what is the HOST? Can `pi.wasm` become a native app (Linux/Windows)? Does it run elsewhere?

## 1. What the HOST is

A WASM module cannot touch the OS. Every system act is an `import` the host fulfills. `pi.wasm` declares exactly three (verified via `wasm-tools print`):

```
(import "wasi_snapshot_preview1" "fd_write" ...)
(import "wasi_snapshot_preview1" "fd_read"  ...)
(import "wasi_snapshot_preview1" "proc_exit" ...)
```

The HOST is whatever loads the module and implements those imports using real syscalls:

| Host | How it maps imports |
|------|---------------------|
| `wasmtime run` (our case) | `fd_write`→`write()`, `fd_read`→`read()`, `proc_exit`→`exit()` on Linux |
| Browser | WASI shim in JS → DOM/console/fetch |
| Python/Node/Rust program | `wasmtime`/`wasmer` library + a few lines of glue |
| Custom C host | your own 3 functions (our pi needs only 3!) |

Sandbox consequence: the module only gets what the host grants (capability model) — no ambient filesystem/network unless the host passes file descriptors/preopens.

## 2. Portability: same file, every OS

`pi.wasm` is OS-agnostic bytecode. It runs unmodified anywhere a WASI runtime exists:

- Linux/macOS/Windows: install that OS's `wasmtime` (or `wasmer`/`WasmEdge`/`WAMR`), `wasmtime run pi.wasm` — identical behavior, console I/O mapped to Win32/POSIX underneath.
- Browsers/edge/embedded: with a WASI shim or `WAMR`.
- No recompilation per platform — that IS the portability payoff vs C/Rust binaries.

Minimal imports (=3) mean maximal portability: any host can satisfy them.

## 3. From `.wasm` to native app (three levels)

1. **Portable artifact (today):** ship `pi.wasm` + "install wasmtime" instructions. One file for all OSes. Verified: same bytes run here.
2. **AOT precompiled (verified 2026-09-04):** `wasmtime compile src/pi.wasm -o pi.cwasm` (18 KB) + `wasmtime run --allow-precompiled pi.cwasm` → identical output (`3.1415926535897932384626433` for N=25). Faster startup, still needs the `wasmtime` binary — not standalone.
3. **True standalone `.exe`:** two options —
   - **Embed:** tiny host (Rust `wasmtime` crate / C `wasm-c-api`, ~50 lines) with `pi.wasm` bytes inside, compiled per OS → one native binary per platform, no runtime install. Standard practice.
   - **Transpile:** `wasm2c` (present here: `/usr/bin/wasm2c`, wabt 1.0.41) converts `pi.wasm` → C source, then `cc` builds a real exe with zero WASM runtime. Catch: someone must implement the 3 imports in C (`write`/`read`/`exit` wrappers — trivial for pi, the reason small import surfaces win).

Recommendation: stay at level 1–2 during development (fast iteration, one artifact), use level 3 (embed or `wasm2c`) at release time when "double-clickable exe, no dependencies" matters.
