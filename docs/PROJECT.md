# Living Project State

Read this document first when starting work in a fresh context. It records
shipped behavior, current gaps, and ordered next work. How to write WAT
against the harness lives in `docs/AUTHORING.md`; the rules in `AGENTS.md`
stay terse on purpose. Update this file with every host capability change.

## Goal

AI-Direct IR lets an AI author application behavior directly in WebAssembly
Text (WAT). `air` is the generic product: it assembles, validates, links,
runs, and packages configured WASM applications. It must not grow a
library-specific or application-specific API merely because an example needs a
dependency.

## Related Projects

| Repository | Role | Work that belongs there |
|---|---|---|
| `ai-direct-ir` | Generic platform and source of truth | `air` runtime, manifest, validation, composition, permissions, lifecycle, packaging, and generic source tooling. |
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

`air` is version `2.0.0`. It embeds the Rust `wat` parser, so an application
needs only `air` on `PATH`. A manifest does not have to declare `target`: the
artifact's preamble says whether it is a component or a Core module, and an
explicit `target` that disagrees is an error. `new` creates and assembles a
starter; `build` forces assembly; `check`, `run`, and `dist` rebuild missing
or stale WAT, validate, then continue. `build` compiles before writing, so a
broken module never overwrites the previous artifact. Errors report against
the authored fragment file and line, not expanded text.

Every example is a WASI 0.2 component; `component` is the default target and
`browser` (Core WASM against a generated Canvas host) the only other one.
The `;; @wasi` directive generates the component boundary from the vendored
WASI 0.2.12 WIT plus `air/wit/ai-direct-host/host.wit`, narrowed to the
application's own imports — see `docs/AUTHORING.md`. `web.*` remains a
builder-phase interface: redesign it directly when the Component Model
provides the correct generic boundary, no shims without a concrete released
consumer.

`[[providers]]` wires a component app to provider components at link time
(runtime linking, no composition tool): the bundle ships app plus providers,
and plain values cross the boundary while resource handles do not. Grants are
`root` / `[[dirs]]` / `--dir` / `--dir-rw` for directories (`write = true`
for state) and `network = true` / `--net` for sockets. `mode = "gui"` picks
the per-frame host loop for a component importing `ai-direct:host/ui`;
`target = "gui"` does not exist.

## Current Verification

```bash
cargo fmt --manifest-path air/Cargo.toml --check
cargo check --manifest-path air/Cargo.toml
cargo test --manifest-path air/Cargo.toml
./build.sh
```

`air/tests/cli.rs` drives the real binary end to end: scaffold, build,
check, run; rejected invalid modules leaving the artifact intact; fragment
errors naming authored source; includes, cycles, `..` paths; named segments
and `;; @data` placement; import-narrowed boundaries; handle drops;
`heap-mark`/`heap-reset` under load; every example checking; `hello`/`pi`
stdout; `tcp-hello` over a real socket with and without the network grant;
`server` static routes, 404/403, provider digest, per-request drops+reset,
`/quit`; `ui`/`term` command-mode boundary crossings. Unit tests cover the
scanner, address parsing, byte-length decoding, and emitted lowerings.

## Current Gaps

- No WIT conformance check in `air`. Nothing verifies that a `[[providers]]`
  artifact exports the world its contract declares; a disagreement surfaces
  as a link failure. `air/wit/ai-direct-host/host.wit` is one file for both
  sides — the shape a provider's WIT should reach.
- No build-time composition: app ships alongside providers, and resource
  handles cannot cross a provider boundary.
- The heap frees in one order or not at all. `heap-mark`/`heap-reset`
  release a whole iteration; no general allocator, no growth past `pages=`.
  Waits on an application stating the requirement (Next Work item 2).
- Validation-error mapping to source lines is effectively Core-module-only:
  multi-module components report the module index against one function-index
  space per module.
- No provider resolver, lockfile, or `air add`. Catalog packages reach
  consumers by hand copy into `vendor/`; nothing checks a copy against the
  catalog or resolves a version.
- A prebuilt Core module cannot be lifted by `air` alone (`wasm-tools
  component new` + Preview 1 adapter). `air init` names the commands; the
  step sits with whoever packages the provider.
- No generic writable WASI data mount, persistence provider, native sidecar,
  or browser provider composition.
- The mail example is a component but still a mock inbox: proposed WIT
  contracts only, no SQLite, IMAP/JMAP, SMTP, TLS, TUI, secrets, account, or
  real mailbox data.

## Next Work

Ordered so each step is provable on its own, and the catalog stops being
specification-only before more specification is written.

1. Point the WIT emitter at a provider's contract. `filesystem`, `sockets`,
   `term`, `ui` are generated; catalog provider WIT and `wasi:clocks` /
   `wasi:random` still go through hand-written declarations. The granularity
   rule (own imports name what to generate), the naming rule (WIT export key
   minus bracketed kind), and `<resource>.drop` are settled; a new *known*
   interface is a table entry in `air/src/wit.rs`. Left is the unknown one:
   a provider WIT arriving as a file a `[[providers]]` entry names, so
   `resolve()` takes a path and the capability table gains a load-time entry.
2. Continue up the memory ladder: records with named fields, then a real
   allocator. Segment placement and per-iteration reset are as far as a bump
   pointer goes. Waits on a long-lived collection stating the requirement —
   grow the mail example until it does.
3. Decide build-time composition only when a released provider needs a single
   fused artifact or handle passing.
4. Add a provider consumer proof in the mail example: a `[[providers]]`
   entry and an imported interface, proving the contract shape before a
   consequential dependency is chosen.
5. After the component path works end to end, present SQLite candidates for
   approval; only then add a generic writable data capability and a
   `mail-store` provider.
6. Keep a standing check on WASI 0.3 rather than scheduling it. `wasmtime-wasi`
   gates p3 behind a non-default feature as experimental and incomplete, with
   no sync linker while `air` is synchronous throughout — adopt when p3 drops
   the caveat, becomes default, or ships a `wasi:http` a real app wants.

## Design Rules

- New app needs go in the manifest, never in `air` code. New ABI shapes
  extend the host once, for every app. Host policy (argv, preopens, clock
  behavior, socket grants) is genuinely the harness's job; WASI defines the
  interface, not the answer.
- Builder phase: replace experimental interfaces instead of layering on
  them. No compatibility shims without a real consumer.

## Maintenance

- Never install, upgrade, or remove software without explicit user consent.
- Work from source and documentation, never generated `.wasm`, `dist/`, local
  credentials, or private data.
- Keep the smallest change correct. Preserve compatibility only for a concrete
  active consumer or explicit release commitment.
- Before claiming completion, run the relevant build, `air check`, target
  behavior check, and distribution check when packaging changes.
- Never commit or push without an explicit request. Finishing a unit of work is
  not a request. This repeats `AGENTS.md` deliberately: the two must agree.
