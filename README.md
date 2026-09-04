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

`host-rs build` assembles, validates, *and* compiles the module before writing
the artifact, so a build never leaves a broken `.wasm` behind. Assembly and
validation errors are reported against the fragment file and line the author
wrote, not against the expanded text. Build progress goes to stderr, so an
app's piped stdout stays the app's alone.

Naming a data segment — `(data $msg (i32.const 0x1000) "...")` — gets you
`$msg.ptr` and `$msg.len` from the harness, so a string's byte count is never
written by hand and can never go stale.

A prebuilt component needs only `host-rs` to check, run, or distribute. How a
root component gets composed with prebuilt provider components is still an open
decision; see the `wasm-tools` boundary below.

### Changing The Harness

Changing this repository requires Rust/Cargo compatible with edition 2024 and
Git. `wasm-tools 1.257.1` is an optional cross-check, not a build requirement.
Build and verify with:

```bash
cargo fmt --manifest-path host-rs/Cargo.toml
cargo check --manifest-path host-rs/Cargo.toml
cargo test --manifest-path host-rs/Cargo.toml
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

The embedded `wat` parser already handles the Component Model text format, so
`host-rs` can assemble a `(component ...)` source in-process with no external
tool. Wasmtime 48 already carries the Component Model and WASI 0.2 through the
existing dependency graph. Component *authoring* therefore needs nothing new.

`wasm-tools` remains useful only for checks the harness does not implement:

| Command | Harness use |
|---|---|
| `wasm-tools validate` | Cross-check a Core WASM or Component artifact. |
| `wasm-tools component wit` | Parse and validate a provider's WIT package. |
| `wasm-tools component targets` | Confirm that a component implements the declared WIT world. |

**Composition is an open decision, deliberately deferred.** The component text
format has no form for embedding a prebuilt `.wasm`, so wiring a root component
to vendored provider *binaries* needs something more than the `wat` parser.
`wasm-tools compose` still runs in 1.257.1 but announces `has been deprecated.
Please use wac instead.`, so it is not a foundation to build on. The options are
an external `wac` CLI, an in-process composition crate, or emitting the
composition ourselves. None of them is worth adopting before a real provider
exists to compose; the decision waits for that provider.

`host-rs` will load and execute a completed component through Wasmtime and
WASI 0.2. It will not fetch providers at runtime or place any build tool in
`dist/`.

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
- `host-rs/tests/cli.rs` — end-to-end tests that run the real binary
- `examples/{hello,pi,server,prompts,prompts-raw,gui-hello}/` — runnable apps; each manifest declares its `.wat` source, so the tracked `.wasm` is rebuilt from it
- `libs/http/` — hand-written WAT lib; `libs/sha256/` and `libs/text-width/` — Rust crates wrapping crates.io `sha2` and `unicode-width`
- `native/` — wasm2c experiments; `tools/` — retired Python host (reference); `docs/PROJECT.md` — living project state

Start with `docs/PROJECT.md`. It is the living project documentation
shared with dependent sibling projects: it records shipped behavior, current
limitations, active Core interfaces, and ordered next milestones. Read
`AGENTS.md` for repository rules.
