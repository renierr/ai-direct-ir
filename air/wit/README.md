# Vendored WASI 0.2.12 WIT

Source: the `wasmtime-wasi 48.0.1` crate's `src/p2/wit/deps/` (itself
transcribed from the upstream WASI proposals), copied verbatim:

- `io.wit` — `wasi:io@0.2.12` (needed for `input-stream`, `output-stream`,
  `error`, which `wasi:filesystem` uses)
- `clocks.wit` — `wasi:clocks@0.2.12` (needed for `datetime`, which
  `wasi:filesystem` uses)
- `filesystem.wit` — `wasi:filesystem@0.2.12`

`air` parses these with `wit-parser` and generates the component-WAT
boundary from them, so an application never hand-transcribes a WASI
interface. The WIT text is the source of truth; `air/src/wit.rs` derives.

License of the WIT files: Apache-2.0 WITH LLVM-exception (upstream WASI).
