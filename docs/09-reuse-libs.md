# Reusing Other Languages' Libraries in WASM

Question (2026-09-04): we don't want to reimplement webservers etc. in raw WAT. Can we leverage existing libs? Answer: yes — three routes, verified against this machine (Omarchy, `wasmtime 48`, `clang 22`, `rustc 1.98`).

## Route A — Host provides the hard stuff, WASM provides the logic (works today)

```
Python/Rust/Node host (HTTP server, files, DB drivers)
  └─ imports our AI-generated core.wasm (pure compute: pi, routing logic, templates)
```

- The webserver is NOT in WASM at all. AI generates only the part that benefits from direct-IR (hot logic), host reuses its full ecosystem (`pip`/`cargo`/`npm`).
- This is our `docs/03` "Option C: Embedded". Fastest path to a real web app: e.g. Python `wasmtime` host + Flask/FastAPI serving results computed by `pi.wasm`-style modules.
- Tradeoff: final artifact is host+binary, not a single `.wasm`. Pragmatic, not pure.

## Route B — WASI-HTTP component, server comes from the runtime (WASM-native)

- Target `wasi:http/incoming-handler` (WASI 0.2 Component Model): module exports one `handle(request) -> response` function, runtime IS the server:
  ```bash
  wasmtime serve -S cli -a 127.0.0.1:8080 app.component.wasm
  ```
  (`wasmtime serve` exists in our installed 48.0.1 — confirmed via `wasmtime --help`.)
- AI never writes socket code: no TCP, no TLS, no HTTP parsing. Just request-in/response-out.
- Libraries reused: anything compilable to a component — Rust (`wit-bindgen` + `wasi:http` handler crates), Python (`componentize-py`), JS (`componentize-js`), C (`wasi-sdk`). Plus frameworks that abstract it: **Fermyon Spin** (write handler in Rust/Go/Python/JS, deploy as WASM, `spin up` is the server).
- Tradeoff: needs Component Model toolchain (`wasm-tools component`, WIT definitions, `wasi:http` world). Newer, more moving parts than preview1. This is our Phase 3 target.

## Route C — Compile a C/Rust library INTO the module (self-contained .wasm)

- Single-file C servers (e.g. **mongoose.c**, civetweb) or Rust crates compile to `wasm32-wasip1/wasip2` via `clang --target=wasm32-wasi` (+ `wasi-sdk` sysroot) or `rustc --target wasm32-wasip2`.
- Sockets come from `wasi:sockets` (0.2) or preview1 `sock_*`; TLS via embedded mbedTLS/wolfSSL compiled in.
- We have `clang 22` + `rustc 1.98` locally — but NOT the `wasi-sdk` sysroot / WASI Rust targets (would need `rustup target add wasm32-wasip2`, ask user first per our sudo/install rule).
- Tradeoff: biggest binaries, C-library CVE surface, hardest build. Only worth it when the artifact must be one portable file with no host.

## Recommendation for this project

1. Now: Route A for demos (Python host serves HTTP, AI-WASM does compute). Zero new deps.
2. Next real milestone: Route B minimal handler — AI writes a `wasi:http` component returning pi digits, run under `wasmtime serve`. Reuses the entire server stack without writing it.
3. Route C only on demand (self-contained binary requirement).

Never rewrite in WAT what a host import or component already solves — AI-direct IR should generate the *differentiating* logic and import the commodity (sockets, HTTP, TLS) from below.
