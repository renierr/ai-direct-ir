# Living Project State

Read this document first when starting work in a fresh context. It is living
project documentation for this harness and its dependent sibling projects. Keep
it current as implementation, active contracts, or next work changes.

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

`air` is version `1.0.2`. It embeds the Rust `wat` parser, so an
application needs only `air` on `PATH`, whether it is Core WAT or a
component. A manifest does not have to declare `target`: the artifact's
preamble says whether it is a component (layer `0d 00`) or a Core module
(layer `01 00`), and an explicit `target` that disagrees is an error rather
than a confusing failure inside the wrong linker. `new` creates and assembles a
starter; `build` forces assembly; `check`, `run`, and `dist` rebuild missing or
stale root/fragment WAT, validate, then continue.

`build` assembles, validates, and compiles before it writes. A module that
fails either step leaves the previous artifact untouched, so a broken `.wasm`
can never reach `check`, `dist`, or a commit. Assembly and validation errors are
reported against the fragment file and line the author wrote: `air` owns the
include expansion, so it is the only component that can translate a parser
line or a Core function index back to authored source.

| Target | Current capability boundary |
|---|---|
| `native` | Wasmtime, WASI Preview 1, experimental `term.*`/`net.*`, and declared Core providers. |
| `browser` | Generated Canvas `web.*` host; no provider composition. |
| `gui` | Native egui `ui.*` host and declared Core providers. |
| `component` | WASM Component + WASI 0.2 through Wasmtime's component linker. Source is a `(component ...)` WAT or a prebuilt component. Consumes provider components through `[[providers]]`. **The default for new projects.** |

`hello`, `pi`, and `prompts` are WASI 0.2 components. The other three stay on
Core WASM, for three different reasons worth keeping straight:

None of the three is blocked by WASI lacking an interface, which is what an
earlier version of this document claimed:

- `server` needs its `[[libs]]`/`[[bridges]]` providers as *components*. Those
  entries link prebuilt Core modules by sharing raw memory, which a component
  cannot do. `wasi:sockets` and `wasi:filesystem` exist, so the rest is a
  rewrite.
- `prompts-raw` needs `term.*`, which components can now import as
  `ai-direct:host/term`, and `bridge.text_width`, which is a prebuilt Core
  module from a Rust crate. The bridge is what is left.
- `gui-hello` needs `ui.*` as a value-based interface. `ui.*` is a
  project-owned egui ABI either way; only its pointer-passing signatures stop
  it from crossing.

Preview 1 is therefore not deprecated. It stays the path for Core providers,
and for host ABIs that still pass raw pointers.

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

`air` inserts ordered, project-local relative fragments before parsing.
Fragments may include further fragments; a cycle is rejected by name. Every
include path resolves against the *root* source's directory at any depth, so a
nested fragment reads exactly like a top-level one and never needs `..`. An
include cannot be absolute or contain `..`; fragments must not add another
`(module ...)`. This is source organization, not provider composition. A
separate Core WASM module remains a declared provider with an explicit ABI.

### Named Data Segments

A string in Core WAT needs a pointer and a byte count, and a hand-written count
goes stale on the next text edit without ever failing validation. Naming the
segment moves the count to the harness:

```wat
(func (export "_start")
  (call $print (global.get $banner.ptr) (global.get $banner.len)))

(data $banner (i32.const 0x1000) "  AI-Direct Mail\n" "  ----\n")
```

For every `(data $name (i32.const <addr>) "...")` at module level, `air`
appends `(global $name.ptr i32 ...)` and `(global $name.len i32 ...)` before
parsing. The length is the decoded byte count, so `\n`, `\1b`, `\u{25c6}`, and
literal multi-byte characters all measure correctly; an escape the harness
cannot measure is an error rather than a guess. Named segments must place
themselves at a literal offset, and two of them may not overlap.

Named segments work in a plain `(module ...)` app and in a `(core module ...)`
inside a component; each module's segments are checked for overlap against that
module's own memory.

Unnamed segments are untouched, so naming is the opt-in.

### Harness-Placed Segments

The length was only half of it. An author who names a segment still had to
assign its address, and because segments pack tightly, each address depended on
the previous string's length — so inserting one word moved every string after
it. Declaring a region hands that over:

```wat
;; @data 0x1000..0x8000
(data $intro "\u{25c6} prompts demo\n")
(data $ask-name "\u{25c7} Project name? ")
```

A named segment with no offset is placed inside the region, packed in source
order; a named segment that states an offset keeps it. The memory map stays
author-owned — the region is the one range handed over, not the whole memory,
because the harness cannot see the scratch addresses, buffers and `[[libs]]` ABI
maps an application also uses. Three things are errors rather than guesses: an
unplaced segment with no region declared, a region too small for its segments,
and a region that would run over a segment the author placed.

Converting `examples/prompts/prompts.wat` removed 29 hand-assigned addresses and
29 hand-written lengths — and fixed two live bugs the conversion exposed. The
audit found `\u{2716} Cancelled.\n` printed with a stated length of 14 against an
actual 15, so the program silently dropped the trailing newline, and the
input-closed message read 25 bytes of a 24-byte string. Both had passed
validation, run correctly enough to ship, and survived every previous reading of
the file. That is the whole argument for moving these numbers into the harness,
stated by the example that had them.

### The Generated WASI Boundary

Declaring the WASI 0.2 interfaces, lowering them into Core functions and
exposing a shared memory was the same ~55 lines in every component, and the
most error-prone lines in the repository: a signature that names a local type
id instead of the exported one rejects the whole instance, and the message
does not say why. One directive replaces all of it:

```wat
(component
  ;; @wasi stdin stdout stderr exit pages=2 heap=0x8000
```

Capabilities are `stdin`, `stdout`, `stderr` and `exit`. `pages=` (default 1)
sizes the memory and `heap=` (default `0x8000`) places the canonical ABI bump
allocator above the application's fixed addresses. An unknown word is an error,
not a silent omission, and a second directive is rejected rather than left to
fail as a duplicate identifier in generated text.

`air` emits only what was asked for: `exit` alone pulls in neither
`wasi:io/streams` nor `wasi:io/error`, and `stdout` with `stderr` share one
output stream and one lowered `write`. The generated names are the boundary's
ABI, so the application can rely on them:

| Name | What it is |
| --- | --- |
| `$mem` | core instance exporting `memory` — `(with "env" (instance $mem))` |
| `$wasi` | core instance of lowered imports — `(with "wasi" (instance $wasi))` |
| `$memory` / `$realloc` | the memory and its bump allocator, for lowering further imports |

`$wasi` exports one Core function per capability: `get-stdin`, `read`,
`get-stdout`, `get-stderr`, `write`, `exit`. Everything below the directive is
ordinary Core WAT.

Every generated line reports the directive as its origin, so a validator
complaint about the boundary points at the line the author wrote rather than at
text they never saw.

Converting the four component sources removed 208 lines and changed no
behavior:

| Source | Before | After |
| --- | --- | --- |
| `examples/hello/hello.wat` | 96 | 42 |
| `examples/pi/pi.wat` | 329 | 253 |
| `examples/prompts/prompts.wat` | 439 | 364 |
| `examples/provider-demo/consumer.wat` | 93 | 39 |

This is the same argument that produced named data segments, applied to the
next hand-maintained detail. It also changes what an AI has to know: the
Component Model text format is thinly represented in training data, which is
exactly why these lines were copied between examples rather than written. A
generated boundary removes the need for that knowledge instead of documenting
it. Interfaces that are not WASI — a provider's, or the project's own
`ai-direct:host/term` — are still declared by hand; they are one import and one
lowering, not a type graph.

New projects contain `src/README.md` and the generic
`.agents/skills/ai-direct-ir/SKILL.md`. The skill covers WAT/WASM/WIT/provider
workflow and environment rules; project behavior belongs in its docs and
`AGENTS.md`.

## Current Verification

The current harness implementation has been verified with:

```bash
cargo fmt --manifest-path air/Cargo.toml --check
cargo check --manifest-path air/Cargo.toml
cargo test --manifest-path air/Cargo.toml
./build.sh
```

`air/tests/cli.rs` drives the real binary end to end: scaffold, build,
check, run; a rejected invalid module with the previous artifact intact;
assembly and validation errors naming the authored fragment; nested includes;
rejected cycles, `..` paths, and missing fragments; named data segments
supplying their own pointer and length across escapes and multi-byte
characters, following a text edit, and rejecting overlaps and computed offsets;
build progress staying off the application's stdout; every repository example
checking; and the `hello` and `pi` examples producing their expected stdout.
Unit tests cover the module scanner, address parsing, and byte-length decoding.

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

- No WIT conformance check (`wasm-tools component targets`) in `air`.
- No build-time composition, so a component app ships alongside its providers
  rather than as one fused artifact, and resource handles cannot cross a
  provider boundary.
- `ui.*` and `net.*` are not available to components: their signatures pass
  raw pointers and need value-based replacements first.
- Validation-error mapping to source lines is Core-module-only in practice: a
  component with several core modules reports the module index, but the include
  map only tracks one function-index space per module.
- No project-local component composition. The component text format cannot
  embed a prebuilt `.wasm`, and `wasm-tools compose` is deprecated upstream, so
  the mechanism is an open decision (see Intended Direction).
- No released provider package or provider resolver/lockfile/`air add`.
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
  -> one distributable component plus air
```

The Core `[[libs]]`, `[[bridges]]`, `ui.*`, `web.*`, `term.*`, and `net.*`
mechanisms are experimental transitional tools, not the final public provider
format.

### The Component Path Works

**An AI can author a WASI 0.2 component by hand, in WAT, with no bindings
generator and no language toolchain.** This was the open question behind the
whole component plan, and it is now answered by a running program rather than
an argument. Having proved it, the harness took the work over: the boundary
those examples once spelled out is now generated from `;; @wasi` (see The
Generated WASI Boundary), which emits the same WAT an author could have
written. The proof stands; the typing does not have to be repeated.

`examples/hello/`, `examples/pi/`, and `examples/prompts/` are `wasi:cli/command`
components written entirely as component WAT. They declare the `wasi:io/error`,
`wasi:io/streams`, `wasi:cli/stdin`, `wasi:cli/stdout`, `wasi:cli/stderr`, and
`wasi:cli/exit` interfaces — resources, the `stream-error` variant, and
`cabi_realloc` for host-allocated `list<u8>` results included — lower them into
Core functions, run ordinary Core WAT against them, and lift `run` back out as
`wasi:cli/run@0.2.12`. `air` assembles, validates, instantiates, runs, and
packages them. `wasm-tools validate` and `wasm-tools component wit` agree.

`pi` and `prompts` were converted from Preview 1 with their compute and prompt
logic untouched: only the import layer and a handful of call sites changed. The
Core logic an AI writes is unaffected by which WASI generation carries its I/O.

The premise therefore holds at the Component Model boundary, not only the Core
one. It is not effortless: the interface declarations are far heavier than
Preview 1's flat integer imports, and one construct took real debugging (a
function signature must reference the *exported* type id, not the local type
declaration it was defined from, or validation rejects the whole instance). That
argues for `air` eventually generating the boundary from a `.wit` file the
way it now derives `$name.len` — but it is a convenience, not a prerequisite.

The three converted examples each carry an identical ~60-line WASI boundary,
because `;; @include` is project-local and cannot be shared across example
directories. That duplication is the clearest evidence for generating it.

Verified in this tree, with no new dependency:

- `wat 1.258` is pulled with default features, which include `component-model`.
  `air` can already assemble a `(component ...)` source in-process. A
  hand-written component WAT encodes, passes `wasm-tools validate --features
  all`, and yields its WIT world through `wasm-tools component wit`.
- `wasmtime-wasi 48`'s `p1` feature transitively enables `p2`, which enables
  `wasmtime/component-model` and `wasmtime/async`. `wasmtime_wasi::p2` exposes
  `add_to_linker_sync`, so a synchronous component host fits the existing
  blocking design.

The `component` target uses exactly these. It shares the manifest, the CLI, and
the WAT assembler with the Core path and nothing else: `[[libs]]` and
`[[bridges]]` are rejected for a component app, because they are Core WASM
mechanisms with no meaning across a component boundary.

Arbitrary current Core WAT still cannot call a WIT component provider by adding
a Core linker import: WIT requires the canonical ABI and Components use a
distinct linking domain. A component boundary has to be an explicit target, not
a new import namespace bolted onto the Core path.

### Provider Linking, And What Composition Would Still Add

A component app can consume another component today. `[[providers]]` names a
provider component; `air` instantiates it and forwards its exported
functions into the application's linker with `LinkerInstance::func_new`. No
external tool, no new dependency. `examples/provider-demo/` proves it: a string
crosses consumer to host to provider and back.

That is *runtime linking*, not composition, and the difference is what ships:

| | Runtime linking (works now) | Build-time composition |
|---|---|---|
| Needs | nothing | an external composer |
| Ships | app + provider `.wasm` + manifest | one fused `.wasm` |
| Runs under plain `wasmtime run` | no | yes |
| Resource handles across the boundary | no | yes |
| Plain values (`list<u8>`, `string`, records) | yes | yes |

So composition is no longer blocking provider work; it buys a single
distributable artifact and handle passing. When it is wanted, the mechanism is
still open: the component text format has no form for embedding a prebuilt
`.wasm` (`(core module $m binary "...")` is rejected), and `wasm-tools compose`
prints `has been deprecated. Please use wac instead.`, so the candidates are an
external `wac`, an in-process composition crate, or emitting the composition
directly. None is worth adopting until a released provider needs one of the two
things runtime linking cannot do.

### Custom Host Interfaces

A component imports a project-owned interface exactly as it imports a WASI one;
the harness supplies it through the component linker. `ai-direct:host/term`
exposes the terminal capability that Core apps reach through `term.*`.

`ui.*` and `net.*` have not followed, for a reason that is not about WASI:
their Core signatures pass pointers into guest memory, which has no meaning
across a component boundary. They need value-based signatures (`string`,
`list<u8>`) first. That is a redesign, not a blocker.

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

- *WASI Preview 1* (what `air` uses today) is a flat list of POSIX-shaped
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

1. Build the first provider package in `ai-direct-ir-providers`: SHA-256 with
   WIT, provenance, license notice, component artifact, hash, and a conformance
   test. It can be hand-authored the way `examples/provider-demo/` is, so no
   Rust component toolchain is needed; `libs/sha256/` stays the reference to
   check the result against. Runtime provider linking already consumes it.
2. Continue up the memory ladder: records with named fields, then an
   allocator. Segment addresses are handled (see Harness-Placed Segments), but
   an application with dynamic collections needs both, and neither should be
   designed before a real application states its requirements. Grow the mail
   example far enough to state them.
3. Give `ui.*` and `net.*` value-based signatures so components can import
   them, then convert `gui-hello`. `term.*` already made the trip as
   `ai-direct:host/term`.
4. Decide build-time composition only when a released provider needs a single
   fused artifact or handle passing. See Provider Linking above.
5. Add a separate component consumer proof in the mail example. Do not force
   the existing Core WAT app to call a WIT component without an explicit
   component boundary.
6. After the component path works end to end, present SQLite candidates for
   approval; only then add a generic writable data capability and a
   `mail-store` provider.

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
