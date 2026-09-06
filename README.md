# AI-Direct IR — Build Software Without a Programming Language

## Goal

Instead of: `AI -> source code (Rust/C/JS) -> compiler -> IR -> binary`

Do: `AI -> IR (WASM) directly -> runtime / native binary`

WASM is a small, structured, validated, portable IR with a readable WAT form.
The AI writes application behavior directly in WAT; `air` provides the
runtime, composition, validation, and packaging boundary.

## Three Repositories

This repository is the platform. Its sibling repositories have distinct roles:

| Repository | What it is for | What we do there |
|---|---|---|
| `ai-direct-ir` | Generic platform | Build `air`: compile, compose, validate, run, and package AI-authored WASM applications. Define only generic host/runtime behavior. |
| `ai-direct-ir-providers` | Reusable provider catalog | Adapt proven upstream libraries into reproducible WASM/WIT provider packages with provenance, licenses, hashes, and conformance tests. |
| `ai-direct-ir-example-mail` | Integration-driving application | Build a real WAT mail client. Its needs reveal missing generic platform/provider capabilities; it may break while those are redesigned. |

The application never causes a mail-specific harness API. If it needs SQLite,
SMTP, IMAP/JMAP, a TUI, storage permissions, or provider composition, solve it
generically here or in the provider catalog, then consume it from the app.

## Environment

### Build And Run An Application

Current Core WAT projects need only `air` on `PATH`:

| Tool | Why an application needs it |
|---|---|
| `air` | Builds, checks, runs, and packages the project. |

Browser projects also need a browser to use the generated page. A `mode =
"gui"` project needs the native display libraries supported by the distributed
`air` binary.
Application authors do not need Rust, Cargo, a Rust WASM target, or provider
toolchains merely to edit WAT and run `air build`, `check`, `run`, or
`dist`.

`air new` immediately assembles the starter WAT. Later, `air check`,
`run`, and `dist` rebuild a declared WAT source when it is newer than the WASM
artifact or the artifact is missing. `air build` remains the explicit
force-rebuild command.

`air build` assembles, validates, *and* compiles the module before writing
the artifact, so a build never leaves a broken `.wasm` behind. Assembly and
validation errors are reported against the fragment file and line the author
wrote, not against the expanded text. Build progress goes to stderr, so an
app's piped stdout stays the app's alone.

Naming a data segment — `(data $msg (i32.const 0x1000) "...")` — gets you
`$msg.ptr` and `$msg.len` from the harness, so a string's byte count is never
written by hand and can never go stale.

A prebuilt component needs only `air` to check, run, or distribute. Components
and their declared providers are runtime-linked by `air`; build-time fusion of
that checked graph remains an optional future distribution optimization.

### Changing The Harness

Changing this repository requires Rust/Cargo compatible with edition 2024 and
Git. `wasm-tools 1.257.1` is an optional cross-check, not a build requirement.
Build and verify with:

```bash
cargo fmt --manifest-path air/Cargo.toml
cargo check --manifest-path air/Cargo.toml
cargo test --manifest-path air/Cargo.toml
./build.sh
```

### `wasm-tools` Boundary

`air` embeds the Rust `wat` parser to assemble and validate WAT in-process.
It encodes the module; it does not optimize it. Optimization is a separate,
optional future `wasm-opt` stage.

`wasm-tools` is an Apache-2.0 Bytecode Alliance CLI. The repository is
AGPL-3.0-or-later; Apache-2.0 is compatible with GPLv3-family licensing, so it
may be used or distributed with its required notices. We do not bundle it into
`air` or a shipped application: it is a platform-specific 16 MiB build tool
and is unnecessary at runtime.

**`target = "component"` — a WASM component on WASI 0.2 — is the default for
new projects.** Its source is a `(component ...)` WAT the harness assembles
in-process: the embedded `wat` parser handles the Component Model text format
and Wasmtime 48 already carries WASI 0.2, so component authoring needs no
external tool at all.

`examples/{hello,pi,prompts}/` are `wasi:cli/command` components written
entirely by hand in WAT — WASI interfaces, resources, canonical lowering and
lifting included. **An AI can author the Component Model boundary directly**,
which is the same claim this project makes about Core WASM. It is heavier than
WASI Preview 1's flat integer imports, but it needs no bindings generator and
no language toolchain.

A manifest does not have to declare `target`. The artifact's own preamble says
whether it is a component (layer `0d 00`) or a Core module (`01 00`); declaring
one that disagrees is an error rather than a confusing failure later.

`target = "browser"` is the only other target: Core WASM against the generated
Canvas host, which the browser runs rather than `air`. The native WASI Preview
1 host has retired, so a prebuilt Core module from any language is lifted into
a component with `wasm-tools component new` and consumed through
`[[providers]]`. See `docs/PROJECT.md`.

`mode` is a separate question from `target`: `command` enters the app once,
`gui` opens a native window and calls the entry point once per drawn frame. A
GUI app is an ordinary component that imports `ai-direct:host/ui`.

### Providers

A component can consume another component:

```toml
[[providers]]
source = "provider.wat"
path = "provider.wasm"
```

`air` instantiates the provider and forwards its exported functions into
the application's imports at link time. No composition tool, no new dependency
— see `examples/provider-demo/`. The trade is that the bundle ships both
components rather than one fused artifact, and resource handles do not cross
the boundary; plain values do.

Released providers use a package declaration and a committed lock, rather than
a copied provider tree:

```toml
[registry]
source = "https://github.com/renierr/ai-direct-ir-providers.git"

[[providers]]
package = "ai-direct:base64"
version = "0.1.0"
```

Select and lock it deliberately once with `air add ai-direct:base64@0.1.0`.
This writes `air.lock` and caches the reviewed package under
`$XDG_CACHE_HOME/air/providers` (or `~/.cache/air/providers`). `build`,
`check`, `run`, and `dist` verify the locked artifact, metadata, and WIT before
use. With a healthy cache they make no registry request. If the cached package
is missing or corrupt, they restore the exact package from the lock-pinned
registry revision and verify it again; they never select a newer version.
`air add --from <package-dir> <package>@<version>` is the explicit local
override for provider development and has no automatic restoration source.

`air dist` copies only resolved providers into namespaced `providers/` paths
with a release `air.lock`; the resulting bundle needs no provider cache or
registry access.

A component may also import the project's own capabilities under a WIT
interface name, exactly as it imports a WASI one: `ai-direct:host/term` for
raw-mode terminals (`examples/prompts-raw/`) and `ai-direct:host/ui` for the
egui window a `mode = "gui"` app draws into (`examples/gui-hello/`).

`wasm-tools` remains useful only for checks the harness does not implement:

| Command | Harness use |
|---|---|
| `wasm-tools validate` | Cross-check a Core WASM or Component artifact. |
| `wasm-tools component wit` | Parse and validate a provider's WIT package. |
| `wasm-tools component new` | Lift a prebuilt Core module from any language into a component, so `[[providers]]` can consume it. |
| `wasm-tools component targets` | Confirm that a component implements the declared WIT world. |

**Composition is an open decision, deliberately deferred.** The component text
format has no form for embedding a prebuilt `.wasm`, so fusing a root component
to provider *binaries* needs something more than the `wat` parser.
`wasm-tools compose` still runs in 1.257.1 but announces `has been deprecated.
Please use wac instead.`, so it is not a foundation to build on. The options are
an external `wac` CLI, an in-process composition crate, or emitting the
composition ourselves. An in-process `wac-graph` spike fused the flat demo but
produced an invalid server plus SHA-256 graph, so multi-file distribution
remains the known-good default.

`air` will load and execute a completed component through Wasmtime and WASI
0.2. It restores only an already lock-pinned provider when a cache entry is
missing; it does not select or update dependencies at runtime, and it never
places a build tool in `dist/`.

## Build The Harness

The project executable is `air`, the Rust harness that builds, validates,
and runs component or browser WASM projects:

```bash
./build.sh
./dist/air --help
```

`build.sh` runs Cargo against `air/Cargo.toml` and copies the local release
binary to `dist/air`. It also supports configured Rust
cross-targets such as `./build.sh --target x86_64-pc-windows-gnu`; that requires
the target and its linker to be installed separately.

## Layout

- `air/` — the harness (Rust; `src/main.rs` CLI + `manifest`/`component`/`boundary`/`wit`/`asm`/`cmds` modules)
- `air/tests/cli.rs` — end-to-end tests that run the real binary
- `examples/` — all WASI 0.2 components. Each manifest declares its `.wat` source, so the tracked `.wasm` is rebuilt from it
- `native/` — wasm2c experiments; `tools/` — retired Python host (reference); `docs/PROJECT.md` — living project state

Start with `docs/PROJECT.md`. It is the living project documentation
shared with dependent sibling projects: it records shipped behavior, current
limitations, active Core interfaces, and ordered next milestones. Read
`AGENTS.md` for repository rules.
