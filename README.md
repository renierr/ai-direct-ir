# AI-Direct IR — Build Software Without a Programming Language

## Goal

Instead of: `AI -> source code (Rust/C/JS) -> compiler -> IR -> binary`

Do: `AI -> IR (WASM) directly -> runtime / native binary`

WASM is a small, structured, validated, portable IR with a readable WAT form.
The AI writes application behavior directly in WAT; `host-rs` provides the
runtime, composition, validation, and packaging boundary.

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

Current Core WAT projects need only `host-rs` on `PATH`:

| Tool | Why an application needs it |
|---|---|
| `host-rs` | Builds, checks, runs, and packages the project. |

Browser projects also need a browser to use the generated page. GUI projects
need the native display libraries supported by the distributed `host-rs` binary.
Application authors do not need Rust, Cargo, a Rust WASM target, or provider
toolchains merely to edit WAT and run `host-rs build`, `check`, `run`, or
`dist`.

`host-rs new` immediately assembles the starter WAT. Later, `host-rs check`,
`run`, and `dist` rebuild a declared WAT source when it is newer than the WASM
artifact or the artifact is missing. `host-rs build` remains the explicit
force-rebuild command.

The planned Component Model project path adds `wasm-tools 1.257.1` only to the
build machine when `host-rs build` must compose a root component with locally
vendored provider components. A prebuilt composed component needs only
`host-rs` to check, run, or distribute.

### Changing The Harness

Changing this repository requires Rust/Cargo compatible with edition 2024 and
Git. Component Model changes additionally require `wasm-tools 1.257.1`. Build
and verify with:

```bash
cargo fmt --manifest-path host-rs/Cargo.toml
cargo check --manifest-path host-rs/Cargo.toml
./build.sh
```

### `wasm-tools` Boundary

`host-rs` embeds the Rust `wat` parser to assemble and validate WAT in-process.
It encodes the module; it does not optimize it. Optimization is a separate,
optional future `wasm-opt` stage.

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

## Layout

- `host-rs/` — the harness (Rust; `src/main.rs` CLI + `manifest`/`host`/`net`/`link`/`cmds` modules)
- `examples/{hello,pi,server}/` — runnable apps (WAT + tracked `.wasm` + manifest)
- `libs/http/` — hand-written WAT lib; `libs/sha256/` — Rust crate wrapping crates.io `sha2`
- `native/` — wasm2c experiments; `tools/` — retired Python host (reference); `docs/` — findings + lablog

Start with `docs/PROJECT.md`. It is the living project documentation
shared with dependent sibling projects: it records shipped behavior, current
limitations, active Core interfaces, and ordered next milestones. Read
`AGENTS.md` for repository rules.
