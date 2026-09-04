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
| `ai-direct-ir-providers` | Reusable dependency catalog | WIT contracts, upstream adapters, reproducible provider artifacts, provenance, licenses, hashes, and conformance tests. |
| `ai-direct-ir-example-mail` | Consumer and integration driver | WAT application behavior, user flows, state policy, and declared provider consumption. |

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

`host-rs` inserts ordered, project-local relative fragments before parsing. An
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
./build.sh
```

Fresh native, browser, and GUI scaffolds have completed their applicable
`new`, `check`, `run`/`serve`, and `dist` flows. The mail example builds and
runs from its root `mail.wat` plus `src/state.wat` and
`src/views/inbox.wat`; changing an included fragment triggers an automatic
rebuild.

## Current Gaps

- No WASI Preview 2 / Component Model application target in `host-rs`.
- No component-aware manifest fields, component linker, Component Model
  execution path, WIT conformance check, or component distribution workflow.
- No build-time project-local component composition through `wasm-tools`.
- No released provider package or provider resolver/lockfile/`host-rs add`.
- No SHA-256 WIT component proof.
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

`wasm-tools 1.257.1` is installed and Apache-2.0 licensed. It remains an
external build-time tool, not a runtime dependency or a `dist/` artifact. The
planned harness usage is limited to:

| Command | Planned use |
|---|---|
| `wasm-tools validate` | Validate Core WASM or Component artifacts. |
| `wasm-tools component wit` | Parse and validate WIT packages. |
| `wasm-tools component targets` | Verify a component implements its declared WIT world. |
| `wasm-tools compose` | Compose explicit project-local root/provider components into one artifact. |

Wasmtime 48 already provides the required Component Model and WASI Preview 2
APIs through the existing dependency graph. Arbitrary current Core WAT cannot
call a WIT component provider by adding a Core linker import: WIT requires the
canonical ABI and Components use a distinct linking domain.

## Next Work

1. Add an additive component-app manifest kind for a prebuilt
   `wasi:cli/command` component. Keep all existing Core paths intact.
2. Add a dedicated Wasmtime Component/WASI 0.2 linker and make `host-rs check`,
   `run`, and `dist` validate, instantiate, execute, and package that prebuilt
   component.
3. Add declared project-local component provider inputs and invoke
   `wasm-tools compose` during `host-rs build`; validate output and package the
   completed component rather than runtime provider inputs.
4. Build the first small provider proof in `ai-direct-ir-providers`: SHA-256
   with WIT, provenance, license notice, component artifact, hash, and
   conformance test. No SQLite selection is needed for this proof.
5. Add a separate component consumer proof. Do not force the existing Core WAT
   mail app to call a WIT component without an explicit component boundary.
6. After the component path works, present SQLite candidates for approval;
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
