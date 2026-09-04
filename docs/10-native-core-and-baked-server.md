# What Core WASM Provides — and Baking In a Proven HTTP Server

## 1. What WASM natively provides: almost nothing (by design)

Core WebAssembly (2.0/3.0, no WASI, no host) is pure computation in a sandbox:

- Value types: `i32/i64/f32/f64`, `v128` (SIMD), `funcref/externref`
- Linear memory: one byte array, `load/store`, `memory.grow` — no malloc, no strings
- Functions: direct + indirect calls, structured control flow (`block/loop/if/br` — no goto)
- Tables, globals, `start` function, `import/export`, data segments

That is ALL. No print, no files, no clock, no random, no sockets, no threads (threads/SIMD/GC are optional proposals, not system access). Proof sitting in our repo: `src/hello.wasm` cannot print by itself — it must `import "wasi_snapshot_preview1" "fd_write"` from `wasmtime`. Every capability arrives as a host import; WASI is just a standardized *convention* for those imports, not part of the language.

Consequence: a "self-contained webserver.wasm" must contain the server code *inside the module* (compiled C/Rust) and only import raw capabilities (TCP sockets, clocks) from below.

## 2. Goal restated

One `.wasm` artifact, HTTP serving baked in, but server code written by others (proven libs), AI generates only our logic. Pattern: **component composition**.

```
┌─────────────────────────────────────────────┐
│ app.wasm  (ONE file, runs under wasmtime)   │
│                                             │
│  server component (proven lib, e.g. Rust    │
│   std::net or mongoose.c compiled to WASM)  │
│        │ WIT interface: serve(handler)      │
│  handler component (AI-generated, ours:     │
│   pi logic, routing — like pi.wat)          │
│        │ imports                            │
│  wasi:sockets + wasi:clocks (host caps)     │
└─────────────────────────────────────────────┘
```

Compose with `wac plug` / `wasm-tools compose`. No C-level linking, no relocations for the AI to emit — WIT is the ABI boundary, each side stays a plain module. This is Route C from `docs/09` done right.

## 3. Concrete proven-lib candidates

| Lib | Lang | Why it fits | WASM path | Status |
|-----|------|-------------|-----------|--------|
| Rust `std::net::TcpListener` + `http` crate (parsing only, no async runtime) | Rust | `std` networking works on `wasm32-wasip2` (sockets via WASI 0.2); `http` crate does parsing/serialization without I/O | `rustc --target wasm32-wasip2`, export WIT `serve` | Needs `rustup target add wasm32-wasip2` — NOT installed here (checked: only android+host targets). Ask user before installing. |
| `mongoose.c` (single amalgamated C file, MIT) | C | Event-driven HTTP/WS, no deps, designed for embedding | `clang --target=wasm32-wasi` + wasi-sdk sysroot, POSIX sockets map to WASI `sock_*` | Needs wasi-sdk sysroot — not installed. Plausible but unverified; propose as experiment, not fact. |
| `civetweb` / `libmicrohttpd` | C | Proven, but threads + more POSIX surface | Same as above, harder | Backup options |
| Fermyon Spin's `wasi:http` model | any | Inverts correctly IF you accept host-serves — user rejected, noted | n/a | Rejected per requirement |

## 4. Proposed experiment (needs user-approved installs)

1. `rustup target add wasm32-wasip2` (+ `wasi-sdk` only if C route chosen) — ASK FIRST.
2. Build minimal Rust `serve` component: TCP accept loop on 8080, parse with `http` crate, call imported AI handler, serialize response.
3. AI writes handler component (request bytes in → pi/response bytes out), same skill level as `pi.wat`.
4. `wac plug server.wasm handler.wasm -o app.wasm`, `wasmtime run app.wasm`, `curl localhost:8080`.
5. Success = one `.wasm`, server code 100% third-party, app logic 100% AI-IR.

Rule preserved: AI emits the differentiating logic; commodity (sockets/HTTP/TLS) comes from proven code below — just linked *inside* the artifact instead of living in the host.
