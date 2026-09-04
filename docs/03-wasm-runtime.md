# Running WASM — How an AI-Emitted Module Becomes an App

## Mental model

WASM module is pure computation. It cannot print, open files, or use network on its own. The **host** provides imports. WASI is the standard set of imports for apps (like a portable POSIX).

```
app.wasm (AI-generated)
  + imports: wasi_snapshot_preview1.fd_write, environ, clock, fs, sockets
  + host: wasmtime / browser / node / python
  = running app
```

## Option A: Standalone runtime + WASI (primary for this project)

Runtimes:
- `wasmtime` — reference, Bytecode Alliance, supports WASI 0.2 + Component Model + AOT. Use this first.
- `wasmer`, `WasmEdge`, `WAMR` — alternatives (edge, tiny embedded).

Typical commands (to verify):

```bash
# text -> binary
wat2wasm hello.wat -o hello.wasm
wasm-tools validate hello.wasm
wasm-tools print hello.wasm

# run (JIT)
wasmtime run hello.wasm

# optimize
wasm-opt -O3 hello.wasm -o hello.opt.wasm

# ahead-of-time to native
wasmtime compile hello.opt.wasm -o hello.cwasm
wasmtime run --allow-precompiled hello.cwasm
```

WASI versions to be careful about:
- `wasi_snapshot_preview1` (a.k.a. preview1) — stable, widest support. Start here.
- WASI 0.2 / `wasi:http`, `wasi:cli`, `wasi:filesystem` (preview2, component model) — future, for composable components.

## Option B: Browser (for UI apps)

```js
const { instance } = await WebAssembly.instantiateStreaming(fetch("app.wasm"), importObject);
instance.exports.main();
```

Host provides DOM, fetch, canvas. AI would generate core logic as WASM + thin JS glue.

## Option C: Embedded library

Host (Rust/Python/Go/C) loads module, calls exported functions:

```python
# python + wasmtime example (planned)
store, linker, module...
func = instance.exports(store)["add"]
print(func(store, 40, 2))
```

Good when AI generates hot logic only, host handles OS/UI.

## Decision for MVP

Use **Option A with `wasmtime` + `preview1`** for first `hello world`. No bundlers, no JS. Prove: `WAT -> wasm -> run -> stdout`.
