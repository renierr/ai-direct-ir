# Living Project State

Read this document first when starting work in a fresh context. It is living
project documentation for this harness and its dependent sibling projects. Keep
it current as implementation, active contracts, or next work changes.

## Goal

AI-Direct IR lets an AI author application behavior directly in WebAssembly
Text (WAT). `host-rs` is the generic product: it assembles, validates, links,
runs, and packages configured WASM applications. It must not grow a
library-specific or application-specific API merely because an example needs a
dependency.

## Related Projects

| Repository | Role | Work that belongs there |
|---|---|---|
| `ai-direct-ir` | Generic platform and source of truth | `host-rs` runtime, manifest, validation, composition, permissions, lifecycle, packaging, and generic source tooling. |
| `ai-direct-ir-providers` | Reusable dependency catalog (Apache-2.0) | WIT contracts, upstream adapters, reproducible provider artifacts, provenance, licenses, hashes, and conformance tests. |
| `ai-direct-ir-example-mail` | Consumer and integration driver | WAT application behavior, user flows, state policy, and declared provider consumption. |

The catalog is Apache-2.0 while the harness and the example stay
AGPL-3.0-or-later. A provider is vendored *into* a consuming application, so a
copyleft catalog would decide the license of every application that adopts one.
The harness is a host an application runs under, not code it links in.

The mail app may break while it reveals an inadequate generic interface. Solve
the general requirement in the harness or provider catalog; never add a
mail-specific host shortcut. Do not choose or vendor SQLite or another
consequential upstream implementation without explicit user approval after
presenting candidates, licensing, WASM/component path, maintenance/security
tradeoffs, platform limits, and a recommendation.

## Current Harness State

`host-rs` is version `1.0.2`. It embeds the Rust `wat` parser, so a Core WAT
application needs only `host-rs` on `PATH`. `new` creates and assembles a
starter; `build` forces assembly; `check`, `run`, and `dist` rebuild missing or
stale root/fragment WAT, validate, then continue.

`build` assembles, validates, and compiles before it writes. A module that
fails either step leaves the previous artifact untouched, so a broken `.wasm`
can never reach `check`, `dist`, or a commit. Assembly and validation errors are
reported against the fragment file and line the author wrote: `host-rs` owns the
include expansion, so it is the only component that can translate a parser
line or a Core function index back to authored source.

| Target | Current capability boundary |
|---|---|
| `native` | Wasmtime, WASI Preview 1, experimental `term.*`/`net.*`, and declared Core providers. |
| `browser` | Generated Canvas `web.*` host; no provider composition. |
| `gui` | Native egui `ui.*` host and declared Core providers. |

Core project-owned providers currently use experimental `[[libs]]` (shared
memory) or `[[bridges]]` (copying adapter) manifest entries. Their exports are
auto-wired under a project-declared namespace. These, and `ui.*`, `web.*`,
`term.*`, and `net.*`, are builder-phase interfaces: redesign them directly
when the Component Model provides the correct generic boundary. Do not add
shims or compatibility layers without a concrete released consumer.

### Modular WAT Source

One application still compiles to one Core WASM module. The root WAT source owns
the outer `(module ...)`, imports, memory, exports, shared helpers, and source
order. It may include fragments using a standalone line:

```wat
;; @include src/views/inbox.wat
```

`host-rs` inserts ordered, project-local relative fragments before parsing.
Fragments may include further fragments; a cycle is rejected by name. Every
include path resolves against the *root* source's directory at any depth, so a
nested fragment reads exactly like a top-level one and never needs `..`. An
include cannot be absolute or contain `..`; fragments must not add another
`(module ...)`. This is source organization, not provider composition. A
separate Core WASM module remains a declared provider with an explicit ABI.

New projects contain `src/README.md` and the generic
`.agents/skills/ai-direct-ir/SKILL.md`. The skill covers WAT/WASM/WIT/provider
workflow and environment rules; project behavior belongs in its docs and
`AGENTS.md`.

## Current Verification

The current harness implementation has been verified with:

```bash
cargo fmt --manifest-path host-rs/Cargo.toml --check
cargo check --manifest-path host-rs/Cargo.toml
cargo test --manifest-path host-rs/Cargo.toml
./build.sh
```

`host-rs/tests/cli.rs` drives the real binary end to end: scaffold, build,
check, run; a rejected invalid module with the previous artifact intact;
assembly and validation errors naming the authored fragment; nested includes;
rejected cycles, `..` paths, and missing fragments; every repository example
checking; and the `hello` and `pi` examples producing their expected stdout.

Every example manifest now declares its `source`, so the tracked `.wasm` is
rebuilt from the tracked `.wat` instead of drifting from it. `hello`, `pi`, and
`server` were re-verified after that rebuild, including `POST /sha256` against
`sha256sum`.

Fresh native, browser, and GUI scaffolds have completed their applicable
`new`, `check`, `run`/`serve`, and `dist` flows. The mail example builds and
runs from its root `mail.wat` plus `src/state.wat` and
`src/views/inbox.wat`; changing an included fragment triggers an automatic
rebuild.

## Current Gaps

- No WASI Preview 2 / Component Model application target in `host-rs`.
- No component-aware manifest fields, component linker, Component Model
  execution path, WIT conformance check, or component distribution workflow.
- No project-local component composition. The component text format cannot
  embed a prebuilt `.wasm`, and `wasm-tools compose` is deprecated upstream, so
  the mechanism is an open decision (see Intended Direction).
- No released provider package or provider resolver/lockfile/`host-rs add`.
- No SHA-256 WIT component proof. The provider catalog has a complete format
  specification and zero provider packages.
- No generic writable WASI data mount, persistence provider, native sidecar, or
  browser provider composition.
- The mail example remains a Core WASI Preview 1 mock inbox. It has proposed
  WIT contracts only; no SQLite, IMAP/JMAP, SMTP, TLS, TUI, secrets, account,
  or real mailbox data is present.

## Intended Direction

The desired durable architecture is:

```text
WIT interfaces
  -> WASM Components using WASI 0.2 capabilities
  -> project-owned provider components
  -> generic Component Model composition
  -> one distributable component plus host-rs
```

The Core `[[libs]]`, `[[bridges]]`, `ui.*`, `web.*`, `term.*`, and `net.*`
mechanisms are experimental transitional tools, not the final public provider
format.

### What The Component Path Already Has

Verified in this tree, with no new dependency:

- `wat 1.258` is pulled with default features, which include `component-model`.
  `host-rs` can already assemble a `(component ...)` source in-process. A
  hand-written component WAT encodes, passes `wasm-tools validate --features
  all`, and yields its WIT world through `wasm-tools component wit`.
- `wasmtime-wasi 48`'s `p1` feature transitively enables `p2`, which enables
  `wasmtime/component-model` and `wasmtime/async`. `wasmtime_wasi::p2` exposes
  `add_to_linker_sync`, so a synchronous component host fits the existing
  blocking design.

Arbitrary current Core WAT still cannot call a WIT component provider by adding
a Core linker import: WIT requires the canonical ABI and Components use a
distinct linking domain. A component boundary has to be an explicit target, not
a new import namespace bolted onto the Core path.

### The Open Composition Decision

The component text format has **no** form for embedding a prebuilt `.wasm`
(`(core module $m binary "...")` is rejected). So a root component WAT can wire
modules and components it declares inline, but cannot reference a vendored
provider binary. Composing against prebuilt providers needs one of:

| Option | Cost |
|---|---|
| External `wac` CLI | A build-machine tool that is not installed here; breaks "an app needs only `host-rs`". |
| In-process composition crate | A new harness dependency; keeps the single-binary property. |
| Emit the composition ourselves | No dependency, most work, most to maintain. |

`wasm-tools compose` is **not** an option: it still runs in 1.257.1 but prints
`has been deprecated. Please use wac instead.` This decision is deliberately
deferred until a real provider exists to compose. Nothing else in the plan is
blocked by it.

`wasm-tools 1.257.1` stays an optional external cross-check — `validate`,
`component wit`, `component targets` — never a runtime dependency or a `dist/`
artifact.

## Why WASI And The Component Model

A `.wasm` module is pure computation. It cannot read a file, open a socket, or
print, and it has no notion of a string, a record, or a list — only `i32`,
`i64`, `f32`, `f64`, and one flat block of memory. Everything else has to be
handed to it by whatever is hosting it. That is the whole problem this project
keeps running into, and the two standards below are the two halves of the
answer.

**WASI is the standard set of things a host hands to a module.** Without it,
every host invents its own import names, and a module only runs where it was
written to run.

- *WASI Preview 1* (what `host-rs` uses today) is a flat list of POSIX-shaped
  functions on integers: `fd_write`, `fd_read`, `path_open`. It is why
  `examples/hello` prints by storing a pointer and a length at address 0 and
  calling `fd_write`. It works, it is well supported, and it cannot describe
  anything richer than bytes.
- *WASI 0.2 / Preview 2* is the same idea rebuilt on the Component Model:
  capability-typed interfaces (`wasi:filesystem`, `wasi:sockets`,
  `wasi:cli`) described in WIT. A component receives only the capabilities its
  world declares, which is what makes "a provider may receive only what its
  configuration grants" enforceable rather than aspirational.

**The Component Model is the standard way two `.wasm` files talk to each
other.** Core WASM linking shares raw memory and integers, which is exactly what
`[[libs]]` and `[[bridges]]` do today — and why both are marked experimental:

- `[[libs]]` gives a provider *the application's own memory*. Fast, zero-copy,
  and no isolation whatsoever: a buggy provider can corrupt the app.
- `[[bridges]]` copies bytes in and out across the boundary, which is safer, but
  every call shape has to be described by hand in the manifest (`in_ptr`,
  `in_len`, `out_ptr`, `out_len`), and the "interface" is a set of integer
  offsets that nothing can type-check.

The Component Model replaces both with an interface described in WIT — records,
strings, lists, `result<t, e>`, resources — plus a canonical ABI that says
exactly how those cross a boundary. Two components compiled from different
languages by different people link because their WIT worlds match, and each gets
its own memory. That is what `providers/mail-store/wit/mail-store.wit`
is written against, and it is why the catalog insists on WIT rather than a raw
C ABI.

So the direction is: keep Core WAT as the thing an AI writes, and move the
*boundaries* — host capabilities and provider dependencies — from hand-described
integer ABIs to WIT worlds carried by the Component Model. The Core mechanisms
stay until there is a component path that replaces them; they do not become a
compatibility layer.

## Next Work

Ordered so that each step is provable on its own, and so the catalog stops
being specification-only before more specification is written.

1. Remove hand-maintained data lengths from authored WAT. Every `(data ...)`
   paired with a matching `(i32.const <len>)` is a silent-corruption bug waiting
   for the next text edit; it already truncated the mail example's inbox by 233
   bytes, and the `new` template teaches the pattern. A generic source
   affordance in `host-rs` — a named data segment whose length the harness
   emits — removes the whole class. This is the single highest-value harness
   change for AI-authored WAT.
2. Build the first provider package in `ai-direct-ir-providers`: SHA-256 with
   WIT, provenance, license notice, component artifact, hash, and a conformance
   test. `libs/sha256/` already exists as a Rust crate, so the only new work is
   the component and the package discipline. This falsifies the catalog's format
   documents cheaply, before more are written against no evidence.
3. Add an additive component-app manifest kind that accepts either a prebuilt
   component or a `(component ...)` WAT source — the embedded parser already
   handles both. Keep all existing Core paths intact.
4. Add a Wasmtime Component/WASI 0.2 linker (`p2::add_to_linker_sync`) and make
   `host-rs check`, `run`, and `dist` validate, instantiate, execute, and package
   that component. Consume the SHA-256 provider from step 2 as the proof.
5. Decide the composition mechanism only once step 4 needs to wire a root
   component to a prebuilt provider binary. See The Open Composition Decision.
6. Add a separate component consumer proof. Do not force the existing Core WAT
   mail app to call a WIT component without an explicit component boundary.
7. After the component path works, present SQLite candidates for approval;
   only then add a generic writable data capability and `mail-store` provider.

## Maintenance

- Never install, upgrade, or remove software without explicit user consent.
- Work from source and documentation, never generated `.wasm`, `dist/`, local
  credentials, or private data.
- Keep the smallest change correct. Preserve compatibility only for a concrete
  active consumer or explicit release commitment.
- Before claiming completion, run the relevant build, `host-rs check`, target
  behavior check, and distribution check when packaging changes.
- Do not commit or push unless explicitly requested.
