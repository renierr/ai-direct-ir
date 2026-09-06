# Living Project State

Read this document first when starting work in a fresh context. It records
shipped behavior, current gaps, and ordered next work. How to write WAT
against the harness lives in `docs/AUTHORING.md`; the rules in `AGENTS.md`
stay terse on purpose. Update this file with every host capability change.

## Goal

AI-Direct IR lets an AI author application behavior directly in WebAssembly
Text (WAT). `air` is the generic product: it assembles, validates, links,
runs, and packages configured WASM applications. The product is the harness
and the AI-to-IR workflow, not any application. The mail client is the
integration driver: a demanding consumer that exercises the harness and
proves an AI can author a real application as IR, end to end. Work happens
in the mail repo only to validate generic capabilities here or in the
provider catalog. It must not grow a
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

`air dist` deliberately retains that checked multi-file layout. It copies only
the app, its declared providers, the host, manifest, and portable grants -- not
every `.wasm` in a project -- so the bundle is standalone and each provider is
inspectable. Composition into one component is deferred: an in-process spike
with the maintained `wac-graph` library succeeded for the flat provider-demo,
but produced an invalid component for the real server plus SHA-256 provider
graph (`instance not valid to be used as import`). The proven runtime-linking
path is the release default until a representative graph validates and runs as
a standalone fused artifact. One file is a convenience, not a reason to risk a
release path or hide provider provenance.

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

- No full WIT conformance check in `air`. `air check` verifies every
  function the app imports from a provider-exported interface is exported by
  some `[[providers]]` entry, naming the entry, the interface, and the
  missing function — but signatures are still only type-checked at link
  time, and nothing verifies a provider artifact against its contract WIT.
  `air/wit/ai-direct-host/host.wit` is one file for both sides — the shape
  a provider's WIT should reach.
- No build-time composition: app ships alongside providers, and resource
  handles cannot cross a provider boundary.
- The heap frees in one order or not at all. `heap-mark`/`heap-reset`
  release a whole iteration; no general allocator, no growth past `pages=`.
  Waits on an application stating the requirement (Next Work item 2).
- Validation-error mapping to source lines is effectively Core-module-only:
  multi-module components report the module index against one function-index
  space per module.
- Provider resolution is intentionally offline-first. `air add --from
  <released-package-dir> <package>@<version>` verifies the release component,
  its declared SHA-256, provider metadata, and WIT; copies the package into
  `$XDG_CACHE_HOME/air/providers` (or `~/.cache/air/providers`); adds a package
  declaration to `host.toml`; and writes the portable, committed `air.lock`.
  `build`, `check`, `run`, and `dist` resolve only the lock and rehash artifact,
  metadata, and WIT every time. A missing or changed cache entry fails rather
  than fetching or upgrading. `dist` writes a provenance `air.lock` and places
  locked artifacts under collision-proof `providers/<package>-<version>-<hash>`
  paths. There is deliberately no registry fetch, update solver, authentication,
  or network policy yet; prove local catalog consumption through the mail driver
  before adding them.
- A prebuilt Core module cannot be lifted by `air` alone (`wasm-tools
  component new` + Preview 1 adapter). `air init` names the commands; the
  step sits with whoever packages the provider.
- No generic writable WASI data mount, persistence provider, native sidecar,
  or browser provider composition.
- The mail driver is still a mock inbox: proposed WIT contracts only, no
  SQLite, IMAP/JMAP, SMTP, TLS, TUI, secrets, account, or real mailbox data.
  That is a missing test load, not a missing product — each of those is work
  only insofar as it validates a generic capability.

## Next Work

Ordered so each step is provable on its own, and the catalog stops being
specification-only before more specification is written.

1. Generate a provider's boundary from its WIT only when a catalog provider
   makes hand-writing painful. Today's provider imports are one import plus
   one lowering with flat signatures (`sha256sum.wat`, `provider-demo/`); the
   transcription pain that justified the emitter (51-line filesystem,
   39-function sockets) has no provider equivalent, and no consumer is
   blocked. The granularity rule, the naming rule, and `<resource>.drop` are
   settled and carry over unchanged, and a new *known* interface
   (`wasi:clocks`, `wasi:random`) stays a one-table-entry job in
   `air/src/wit.rs` whenever an app first imports one. The trigger for the
   unknown-interface machinery is a provider with
   records/variants/resources in its contract — until then this stays a
   watch item, per the no-machinery-without-a-consumer rule. Note it does
   not close the conformance gap either: verifying a provider artifact
   against its WIT is separate work, and the more valuable of the two.
2. Continue up the memory ladder: records with named fields, then a real
   allocator. Segment placement and per-iteration reset are as far as a bump
   pointer goes. Waits on a long-lived collection stating the requirement —
   drive the mail app's data needs until a bump pointer is insufficient, and
   treat whatever it states as the allocator's specification.
3. Prove the offline-first provider store through the mail driver: install the
   proposed provider package with `air add`, commit its `air.lock`, and build,
   check, run, and dist from the locked cache. This is the contract-shape proof
   for the harness path, not mail functionality. Only then decide registry
   fetch and update commands.
4. Revisit optional build-time composition only when a released provider needs
   one fused artifact or handle passing. `wac-graph` can compose in-process but
   is not a dependency today: its flat-demo output worked, its server+SHA-256
   output was invalid. Require a representative graph that validates and runs
   standalone, lockfile-backed provenance, and explicit resource-handle
   semantics. Keep multi-file `dist` as the correct fallback.
5. After the component path works end to end, present SQLite candidates for
   approval; only then add the generic writable data capability (harness)
   and a `mail-store` provider (catalog). The mail app is the testbed that
   justifies both, not the deliverable.
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
