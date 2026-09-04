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

## Three Repositories

This repository is the platform. Its sibling repositories have distinct roles:

| Repository | What it is for | What we do there |
|---|---|---|
| `ai-direct-ir` | Generic platform | Build `host-rs`: compile, compose, validate, run, and package AI-authored WASM applications. Define only generic host/runtime behavior. |
| `ai-direct-ir-providers` | Reusable provider catalog | Adapt proven upstream libraries into reproducible WASM/WIT provider packages with provenance, licenses, hashes, and conformance tests. |
| `ai-direct-ir-example-mail` | Integration-driving application | Build a real WAT mail client. Its needs reveal missing generic platform/provider capabilities; it may break while those are redesigned. |

The application never causes a mail-specific harness API. If it needs SQLite,
SMTP, IMAP/JMAP, a TUI, storage permissions, or provider composition, solve it
generically here or in the provider catalog, then consume it from the app.

## Environment

### Build And Run An Application

Current Core WAT projects need only these executables on `PATH`:

| Tool | Why an application needs it |
|---|---|
| `host-rs` | Builds, checks, runs, and packages the project. |
| `wat2wasm` from WABT | Assembles a declared `.wat` source into the app's Core WASM artifact. A prebuilt Core artifact does not need it. |

Browser projects also need a browser to use the generated page. GUI projects
need the native display libraries supported by the distributed `host-rs` binary.
Application authors do not need Rust, Cargo, a Rust WASM target, or provider
toolchains merely to edit WAT and run `host-rs build`, `check`, `run`, or
`dist`.

The planned Component Model project path adds `wasm-tools 1.257.1` only to the
build machine when `host-rs build` must compose a root component with locally
vendored provider components. A prebuilt composed component needs only
`host-rs` to check, run, or distribute.

### Changing The Harness

Changing this repository requires Rust/Cargo compatible with edition 2024,
Git, and WABT's `wat2wasm`. Component Model changes additionally require
`wasm-tools 1.257.1`. Build and verify with:

```bash
cargo fmt --manifest-path host-rs/Cargo.toml
cargo check --manifest-path host-rs/Cargo.toml
./build.sh
```

### `wasm-tools` Boundary

`wasm-tools` is an Apache-2.0 Bytecode Alliance CLI. The repository is
AGPL-3.0-or-later; Apache-2.0 is compatible with GPLv3-family licensing, so it
may be used or distributed with its required notices. We do not bundle it into
`host-rs` or a shipped application: it is a platform-specific 16 MiB build tool
and is unnecessary at runtime.

The future component build path invokes only these external subcommands:

| Command | Harness use |
|---|---|
| `wasm-tools validate` | Reject an invalid Core WASM or Component artifact. |
| `wasm-tools component wit` | Parse and validate a provider's WIT package. |
| `wasm-tools component targets` | Confirm that a component implements the declared WIT world. |
| `wasm-tools compose` | Compose a root component with explicit project-local provider components into one distributable component. |

`host-rs` will load and execute the completed component through Wasmtime and
WASI 0.2. It will not fetch providers, invoke `wasm-tools` at runtime, or place
the tool in `dist/`.

## Why start with WASM?

See `docs/02-ir-options.md`. Summary: WASM is small (~200 opcodes), sandboxed, portable, has text form (WAT) that is LLM-friendly and binary form that is optimizable.

## How we run it

See `docs/03-wasm-runtime.md`. Summary: WASM needs a host. `wasmtime + WASI` for CLI/apps, browser for UI, embedded host for libraries.

## Build The Harness

The project executable is `host-rs`, the Rust harness that builds, validates,
and runs native or browser WASM projects:

```bash
./build.sh
./dist/host-rs --help
```

`build.sh` runs Cargo against `host-rs/Cargo.toml` and copies the local release
binary to `dist/host-rs`. It also supports configured Rust
cross-targets such as `./build.sh --target x86_64-pc-windows-gnu`; that requires
the target and its linker to be installed separately.

## Status

- [x] Idea сформулирована / formulated
- [x] `hello.wat -> hello.wasm -> wasmtime run` proof of runtime (152 bytes, `wasmtime 48.0.1`, `wabt 1.0.41`, `binaryen 130`, `wasm-tools 1.257.1`; Python emitter byte-identical to `wat2wasm`)
- [x] AI-generated WAT for pure function
- [x] Interactive WAT app (`examples/pi/pi.wat` — stdin prompt, 0..1000 validation, spigot pi, bit-exact vs Chudnovsky at N=100/1000)
- [ ] AI-generated raw WASM binary + `wasm-opt`
- [x] Native exes via wasm2c (`native/hello-native`, `native/pi-native` — no runtime, N=1000 byte-identical, see `docs/13-wasm2c-native.md`)
- [x] Generic harness `host-rs` — composes and packages project-declared Core WASM providers without rebuilding; built-in native, browser Canvas, and egui GUI capabilities (`docs/19-harness.md`)
- [x] Static file server in IR + finished Rust lib (`sha2` via bridge, `POST /sha256` matches `sha256sum`; see `docs/17-static-server.md`, `docs/18-cargo-libs.md`)
- [ ] WASI Component Model target

## Layout

- `host-rs/` — the harness (Rust; `src/main.rs` CLI + `manifest`/`host`/`net`/`link`/`cmds` modules)
- `examples/{hello,pi,server}/` — runnable apps (WAT + tracked `.wasm` + manifest)
- `libs/http/` — hand-written WAT lib; `libs/sha256/` — Rust crate wrapping crates.io `sha2`
- `native/` — wasm2c experiments; `tools/` — retired Python host (reference); `docs/` — findings + lablog

The host composition model and experimental built-in capability contract live in
`docs/22-abi.md`. New app libraries belong in the project's declared WASM
providers; only a new built-in host import needs a harness ABI change.

See `AGENTS.md` for build/run rules, `docs/04-roadmap.md` and `docs/05-decisions.md`.
