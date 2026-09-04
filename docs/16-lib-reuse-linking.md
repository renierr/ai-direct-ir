# Using other people's libs from IR

Core WebAssembly has **no static linker**: a `.wasm` file is one module, and
`wat2wasm` compiles one text file. So "depending on a library" needs a
linking story. Options, weakest to strongest:

1. **Textual vendoring.** Copy the lib's functions into your `.wat`.
   Zero runtime cost, but it's copy-paste: no versioning, no separate
   compilation, and you must have the source (usually not WAT).
2. **Host-wired multi-module linking (proven here).** Compile lib and app to
   separate `.wasm` files. A host instantiates the lib first and satisfies
   the app's `lib.*` imports from the lib's exports. The lib can be authored
   by anyone, in any language that targets WASM — the import/export
   signatures are the interface. This is exactly how C static linking works,
   minus the archiver: the host is the link step.
3. **Component Model (future standard).** WIT interfaces + `wasm-tools
   compose` link components without a custom host. Right tool when the
   ecosystem matures; overkill while we hand-write modules.

## The pattern (libs/http/http.wasm + examples/server/server.wasm)

The one hard problem: **two modules = two memories** by default, so pointers
can't cross the boundary. Solution: the host creates ONE memory, both modules
import it (`env.memory`), the app re-exports it as `memory` so WASI binds to
it. Instantiation order matters — lib first (it needs only `env` +
syscalls), app second (it needs `lib.*`):

```
host: mem = Memory(2 pages); define("env","memory",mem); define net.*;
lib_inst = instantiate(libs/http/http.wasm)  # env + net.send
define each lib export under "lib"
app_inst = instantiate(examples/server/server.wasm)  # env + net + wasi + lib.*
run(port)
```

The lib's memory map is ABI: lib scratch `0x10000-0x17FFF`, lib data
`0x18000+` (read-only, includes string addresses baked into length
constants). Documented in `libs/http/http.wat` header; the app must respect it.

## Gotchas found while proving it

- WASI preview1 **returns fds via out-pointer**, not multi-value
  (`path_open(..., opened_fd_ptr) -> errno`). Multi-value is post-preview1.
- **Hand-counted string lengths are the #1 bug source.** `mime_for`
  returned 25 for a 24-byte string; the stray NUL poisoned the header block
  and curl silently showed empty headers. Verify with raw bytes (`curl -i`),
  never trust the pretty output alone.
- Host API details that bit: wasmtime-py needs `access_caller=True` to see
  memory in callbacks, `ValType` (not `ValKind`) in current versions, and
  `linker.define(store, module, name, item)` for manual wiring.

## Path for real third-party libs

Same shape, bigger content: compile an existing C/Rust lib to a core module
exporting functions (e.g. a `.wasm` exposing `sha256(data,len,out)`), agree
on the shared-memory contract, wire it in the host. The host never inspects
lib internals — only signatures. Our hand-written `lib/http.wat` stands in
for that future lib: from the app's perspective they are indistinguishable.
