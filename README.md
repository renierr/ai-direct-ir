# AI-Direct IR — Build Software Without a Programming Language

## Idea (2026-09-04)

Instead of: `AI -> source code (Rust/C/JS) -> compiler -> IR -> binary`

Do: `AI -> IR (WASM) directly -> runtime / native binary`

Rationale:
- Programming languages are optimized for humans to read, not for AI to write.
- IRs are smaller, more regular, verifiable. A module either validates or it doesn't.
- Skips parsing, AST, HIR/MIR lowering bugs. Fewer layers = fewer hallucinations.
- Existing optimizers (`wasm-opt`, AOT compilers) can still optimize the AI output.

Normal compiler for reference:
`source text -> lexer/parser -> AST -> HIR/MIR -> LLVM IR / WASM -> machine code (LLVM backend / wasmtime AOT) -> linker -> binary`

We replace the frontend with the AI.

## Why start with WASM?

See `docs/02-ir-options.md`. Summary: WASM is small (~200 opcodes), sandboxed, portable, has text form (WAT) that is LLM-friendly and binary form that is optimizable.

## How we run it

See `docs/03-wasm-runtime.md`. Summary: WASM needs a host. `wasmtime + WASI` for CLI/apps, browser for UI, embedded host for libraries.

## Build The Harness

The project executable is `host-rs`, the Rust harness that builds, validates,
and runs native or browser WASM projects:

```bash
./build.sh
./host-rs/target/release/host-rs --help
```

`build.sh` runs Cargo against `host-rs/Cargo.toml` and writes the local release
binary to `host-rs/target/release/host-rs`. It also supports configured Rust
cross-targets such as `./build.sh --target x86_64-pc-windows-gnu`; that requires
the target and its linker to be installed separately.

## Status

- [x] Idea сформулирована / formulated
- [x] `hello.wat -> hello.wasm -> wasmtime run` proof of runtime (152 bytes, `wasmtime 48.0.1`, `wabt 1.0.41`, `binaryen 130`, `wasm-tools 1.257.1`; Python emitter byte-identical to `wat2wasm`)
- [x] AI-generated WAT for pure function
- [x] Interactive WAT app (`examples/pi/pi.wat` — stdin prompt, 0..1000 validation, spigot pi, bit-exact vs Chudnovsky at N=100/1000)
- [ ] AI-generated raw WASM binary + `wasm-opt`
- [x] Native exes via wasm2c (`native/hello-native`, `native/pi-native` — no runtime, N=1000 byte-identical, see `docs/13-wasm2c-native.md`)
- [x] Generic harness `host-rs` — TOML manifests link + host apps and libs without rebuilding (`docs/19-harness.md`); `run`/`check`/`inspect`/`init`
- [x] Static file server in IR + finished Rust lib (`sha2` via bridge, `POST /sha256` matches `sha256sum`; see `docs/17-static-server.md`, `docs/18-cargo-libs.md`)
- [ ] WASI Component Model target

## Layout

- `host-rs/` — the harness (Rust; `src/main.rs` CLI + `manifest`/`host`/`net`/`link`/`cmds` modules)
- `examples/{hello,pi,server}/` — runnable apps (WAT + tracked `.wasm` + manifest)
- `libs/http/` — hand-written WAT lib; `libs/sha256/` — Rust crate wrapping crates.io `sha2`
- `native/` — wasm2c experiments; `tools/` — retired Python host (reference); `docs/` — findings + lablog

See `AGENTS.md` for build/run rules, `docs/04-roadmap.md` and `docs/05-decisions.md`.
