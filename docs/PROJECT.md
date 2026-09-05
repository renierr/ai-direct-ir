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
| `native` | Wasmtime, WASI Preview 1, experimental `term.*`, and declared Core providers. |
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
and `term.*`, are builder-phase interfaces: redesign them directly
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

Capabilities are `stdin`, `stdout`, `stderr`, `exit`, `exit-with-code` and
`args`. `exit` takes a `result`, so 0 and 1 are the only representable values
and it says only whether the run failed; `exit-with-code` takes a `u8` for a
POSIX-style status. `filesystem` and `sockets` are generated from the vendored
WASI WIT rather than transcribed, narrowed to the functions the application
imports from `"fs"` and `"net"` (see below). `pages=` (default 1)
sizes the memory and `heap=` (default `0x8000`) places the canonical ABI bump
allocator above the application's fixed addresses. An unknown word is an error,
not a silent omission, and a second directive is rejected rather than left to
fail as a duplicate identifier in generated text.

`air` emits only what was asked for: `exit` alone pulls in neither
`wasi:io/streams` nor `wasi:io/error`, and `stdout` with `stderr` share one
output stream and one lowered `write`. `filesystem` implies the stream
*resources* (`read-via-stream` returns an `input-stream`) without the
read/write *methods*, and declares the three filesystem functions `sha256sum`
imports rather than the WIT's twenty-nine; `sockets` implies the methods too,
because an accepted connection *is* an `input-stream` and there is no other way
to read one. The generated names are the boundary's ABI, so the application can
rely on them:

| Name | What it is |
| --- | --- |
| `$mem` | core instance exporting `memory`, and on request `heap-mark` / `heap-reset` — `(with "env" (instance $mem))` |
| `$wasi` | core instance of lowered imports — `(with "wasi" (instance $wasi))` |
| `$memory` / `$realloc` | the memory and its bump allocator, for lowering further imports |
| `$fs` | core instance of lowered `wasi:filesystem` imports — `(with "fs" (instance $fs))`, names such as `"descriptor.open-at"` and `"get-directories"` |
| `$net` | the same for `wasi:sockets` — `(with "net" (instance $net))`, `"create-tcp-socket"`, `"tcp-socket.accept"`, `"pollable.block"` |

`$wasi` exports one Core function per capability: `get-stdin`, `read`,
`get-stdout`, `get-stderr`, `write`, `get-arguments`, `exit`. It also releases
handles: a resource the boundary declares can be dropped by importing
`<resource>.drop` — `"input-stream.drop"` and `"output-stream.drop"` from
`"wasi"`, `"tcp-socket.drop"` and `"descriptor.drop"` from the capability
instances. Everything below the directive is ordinary Core WAT.

Every generated line reports the directive as its origin, so a validator
complaint about the boundary points at the line the author wrote rather than at
text they never saw.

### The Boundary From WIT

The shorthand above stopped at hand-picked capabilities until `filesystem`
proved the next step: `air` parses the vendored WASI 0.2.12 WIT
(`air/wit/wasi-0.2.12/`, copied from `wasmtime-wasi 48`) with `wit-parser`
and emits the imports from it — enum cases, flags, records, variants,
resources and method signatures — plus the aliases, canonical lowerings, and
the core instance an application links. Only the instance names (`$fs-types`,
`$net-tcp`, `$fs`, `$net`, ...) are harness ABI; every type and signature is
the WIT's. `filesystem` and `sockets` are both generated this way; the
difference between them is a table of interface names in `air/src/wit.rs`.

The application says how much of it to generate. `wasi:filesystem` declares 29
functions and a program calls a few, so the `(import "fs" "...")` lines are
read as the request: the boundary declares those functions, the types their
signatures reach, and nothing else. `sha256sum` names three of the 29, and its
artifact went from 11596 bytes to 5959 — 108 bytes more than the 51 lines of
hand-transcription it replaced, for a boundary nobody maintains. A `"fs"` name
the WIT does not have is a build error that names the typo, and `filesystem`
with no `"fs"` import at all is an error too, rather than an empty instance to
puzzle over.

That is why expansion is two passes (`air/src/asm/source.rs`): the `;; @wasi`
line holds its place while includes expand, and the boundary is spliced in once
every module has said what it imports — from any fragment, at any include
depth. Generated lines still report the directive as their origin, so a
validator complaint about the boundary points at the line the author wrote.

Converting `examples/sha256sum/sha256sum.wat` deleted 51 hand-transcribed
lines for one directive word, `filesystem`, with no change to the application's
`"fs"` imports and a byte-identical digest. The emitter (`air/src/wit.rs`) is
generic over resources, records, variants, enums, flags, tuples, options,
results, lists, borrows, and owns. Provider WIT (`ai-direct:sha256/digest`),
`wasi:clocks` and `wasi:random` still wait for the same treatment.

#### What sockets changed

`wasi:sockets` is the interface that tested whether the approach generalises,
and it moved three things.

**Names are qualified by resource.** `wasi:filesystem` has no two methods
sharing a short name, so `"open-at"` was unambiguous. `wasi:sockets` has 39
functions with 10 collisions — `subscribe` alone belongs to five different
resources. The rule is now that an import name is the WIT export key with its
bracketed kind dropped: `descriptor.open-at`, `tcp-socket.subscribe`,
`get-directories`. It is unique across a package by construction, needs no
second rule, and cost `sha256sum` three edited import lines. Its artifact went
from 5959 to 6033 bytes: longer export names, plus the per-interface prefix the
generic emitter gives its generated identifiers (`$fs-types-descriptor-open-at`
rather than `$fs-descriptor-open-at`), which lands in the name section.

**Interfaces share their declarations.** `wasi:sockets` spreads a TCP listener
over `wasi:io/poll`, `wasi:sockets/network`, `instance-network`, `tcp` and
`tcp-create-socket`. `error-code`, `network` and `pollable` are each declared
once, by the interface that owns them, and `(alias export ...)`-ed into the
others — the general form of the trick the `wasi:filesystem/preopens`
`descriptor` re-export needed, which is no longer a special case in the code.
An interface that contributes only a type contributes no functions:
`wasi:clocks/monotonic-clock` supplies `duration` to the keep-alive setters
without making clock reads part of the sockets capability.

**Sockets are a host grant.** `air` links the whole WASI 0.2 set, but
`wasmtime`'s `WasiCtxBuilder` disables TCP, UDP and name lookup by default, so
before this every `wasi:sockets` call would have answered `access-denied`.
`network = true` in the manifest, or `air run --net`, is the grant — the same
"nothing is reachable unless it asks" rule the directory grants follow, and the
same category as `--dir`: WASI defines the interface, not the answer.

`examples/tcp-hello/` is the proof: it binds 127.0.0.1:8125, blocks on a
`pollable`, accepts connections in a loop, answers on each accepted
`output-stream`, releases the three handles the accept handed it, and stops on
`GET /quit`. It imports nine of the 39 functions plus two drops, and its
artifact is 6567 bytes. `air/tests/cli.rs` drives it over a real socket for two
connections — the second is served only because the first was released — and
checks that removing the grant stops the run at `create-tcp-socket` with
error-code 1.

#### Releasing handles

A handle is the one thing a component holds that the canonical ABI cannot give
back for it. Lowering and lifting cross linear memory automatically; a resource
handle is a table entry, and only the program that owns it knows when it is
done. So every resource a boundary declares also offers `<resource>.drop`:

```wat
(import "net" "tcp-socket.drop" (func $drop_socket (param i32)))
(import "wasi" "input-stream.drop" (func $drop_in (param i32)))
```

It is spelled like a method because it reads like one, but it is
`(canon resource.drop <type>)` — a canonical builtin, not a WIT function, and
the one entry in a capability instance with no WIT export key behind it. It
takes no memory or realloc: a handle is one `i32` in and nothing out.

Two rules keep it in line with the rest of the boundary. It is opt-in by
import, like every generated function, so no artifact grew: rebuilding all nine
examples after this landed changed only `tcp-hello.wasm`. And a drop for a
resource the boundary never declared is a build error naming the line, rather
than an unresolved core import discovered at link time.

The stream resources are the one case that crosses instances. `input-stream`
and `output-stream` are declared by the hand-written `wasi:io` part of the
boundary, because stdio hands them out too, so their drops live in `$wasi`
while `tcp-socket.drop` lives in `$net`. That makes `$wasi` the one instance
whose contents are decided by both the directive and the imports — deliberately:
dropping is not a capability, it releases a handle the program already holds,
and only the program can say which.

`examples/tcp-hello/` is what needed this. An accepted connection is three
owned handles, and dropping the socket is what closes the TCP connection, so
the drops are load bearing rather than tidy — without them the first client
never sees end of stream and the loop leaks a handle per request. Its artifact
went from 5859 to 6567 bytes for the accept loop, the `/quit` check, and the
three drops.

#### Releasing memory

Handles were not the only thing a loop had to give back. The boundary's
`cabi_realloc` is a bump pointer: the canonical ABI allocates every
host-produced value through it — a read buffer, an argument list — and nothing
ever frees. That is right for a run with an end, and wrong for the accept loop
the previous section just made possible. Hammered with `curl`,
`examples/tcp-hello/` died on request 420:

```
Caused by: realloc return: beyond end of memory
```

The heap is one pointer, so releasing an iteration's allocations is putting it
back. `$mem` gained two exports for it:

```wat
(import "env" "heap-mark" (func $heap_mark (result i32)))
(import "env" "heap-reset" (func $heap_reset (param i32)))
```

Mark at the top of the loop, reset at the bottom, and the whole run costs one
connection's allocation. The same example now serves 5000 requests and exits 0.

This is deliberately not an allocator. It frees in the reverse of the order it
allocated or not at all, which is exactly the shape a request loop has and no
help at all to a long-lived collection — that is still Next Work item 2, and
still waiting on an application that states the requirement. What it does buy
is that "a component that stays up" stopped being blocked on a design nobody
has the inputs for yet.

Both controls are opt-in by import, like every other generated name, so no
artifact grew: rebuilding all nine examples changed only `tcp-hello.wasm`.
`$mem-mod` exports a closed set of four names, which is what lets a misspelled
`"env"` import be a build error naming the line rather than an unresolved core
import. The reset takes a mark from `heap-mark` and nothing else — it is not
validated, because a guest can already write anywhere in its own memory, and a
clamp would trade one corruption for another rather than catching the bug.

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
`sha256sum`. `sha256sum` has since dropped its 51-line hand-transcribed
`wasi:filesystem` block for `;; @wasi ... filesystem`: the boundary is
generated from the vendored WIT, the digest still matches `sha256sum` byte
for byte, and unit tests pin the 37-case `error-code` enum plus the `$fs`
lowerings while `air/tests/cli.rs` pins the end-to-end import, link, and run.
Narrowing that boundary to the application's own `"fs"` imports halved the
artifact (11596 to 5959 bytes) with the digest, the error paths, and the
line-accurate diagnostics all re-verified afterwards.

`examples/tcp-hello/` extends the same machinery to `wasi:sockets`: `air check`
passes, `air run` binds 127.0.0.1:8125 and answers a real `curl`, and
`air/tests/cli.rs` drives it over a `TcpStream` and asserts that removing
`network = true` stops the run at `create-tcp-socket` with error-code 1. Unit
tests pin that the boundary declares nine of the WIT's 39 functions, shares one
`error-code` declaration across the TCP interfaces, and carries no UDP.

Handle release was verified the same way. The example now accepts in a loop and
drops the accepted socket and its two streams each time; `air/tests/cli.rs`
serves two connections and reads each to end of stream, which only arrives
because the socket was dropped. Commenting the socket drop out was checked
directly: a client reading to end of stream waits 4s and times out instead of
finishing in 3ms. Unit tests pin the emitted `(canon resource.drop ...)`, that
a drop alone declares its resource without dragging in the interface's other
types, and that a drop for an undeclared resource is an error. Rebuilding every
example afterwards changed only `tcp-hello.wasm`.

The bump heap was measured the same way. `examples/tcp-hello/` died on request
420 with `realloc return: beyond end of memory`; with `heap-mark`/`heap-reset`
it serves 5000 and exits 0. `air/tests/cli.rs` pins that as 400 requests padded
to ~450 bytes each — 180KiB against a 32KiB heap, so the assertion is in bytes
rather than in a request count that a terser request line would slip under. The
test was checked against its own regression: with the reset commented out it
fails on request 73. Unit tests pin that both controls are opt-in and that an
unknown `"env"` import is an error.

`examples/server/` is now a component and has end-to-end coverage it never had
as a Core app -- it was only ever `air check`ed and driven by hand.
`air/tests/cli.rs` serves `www/index.html` and `www/style.css` with the MIME
types their extensions imply, repeats a request on a second connection, gets
404 for a missing file and 403 for a `..` escape, digests `abc` through the
vendored provider to `ba7816bf...`, runs 300 padded requests to exercise the
per-request descriptor and stream drops together with the heap reset, and stops
on `/quit` with exit 0. Benchmarked at 17.3k req/s on the in-memory route and
10.2k on `index.html` -- see Retiring net.*.

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
- `ui.*` is not available to components: its signatures pass raw pointers and
  need value-based replacements first.
- The heap frees in one order or not at all. `heap-mark`/`heap-reset` release a
  whole iteration at once, which is what a request loop needs and nothing a
  long-lived collection can use. There is still no general allocator and no
  growth past the `pages=` the directive asks for. See Releasing Memory and
  Next Work item 2.
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

The Core `[[libs]]`, `[[bridges]]`, `ui.*`, `web.*`, and `term.*` mechanisms
are experimental transitional tools, not the final public provider format.
`net.*` was one of them and is the first to have finished the trip: it is
gone.

### Retiring net.*

`net.*` was five host functions over `std::net` -- listen, accept, recv, send,
close -- with an integer handle table in `Host`. It existed because Core WASM
had no sockets. `wasi:sockets` is that capability as a standard, so the two
were doing the same job, and the question was only whether the standard was
good enough to delete the proprietary one.

It was measured before it was decided. Both servers are single-threaded and
close after each response, so the fair comparison is serial
connection-per-request. `examples/server/` was given an in-memory `/hello`
route emitting the same 111 bytes `examples/tcp-hello/` sends, so the
measurement was of the two transports rather than one transport plus a
`path_open`:

| Path | Core + `net.*` | Component + `wasi:sockets` |
| --- | --- | --- |
| in-memory response | 24.5k req/s, p50 37us | 18.0k req/s, p50 53us |
| `www/index.html` | 9.1k req/s, p50 100us | 10.2k req/s, p50 91us |

The component costs ~16us per request on the synthetic path: roughly ten
boundary crossings against four (`pollable.block`, `accept`, `blocking-read`
plus its `cabi_realloc` call back into the guest, `blocking-write-and-flush`,
three drops, and the two heap controls), at about 2.7us each.

The second row is the one that decided it. On the path the server actually
exists to serve, the component is *faster* -- it buffers one response and
issues one write, where the Core version called `net.send` per header. The
16us is real and irrelevant: the file read alone costs four times more.

Three things settled it beyond the numbers:

- **`net.*` had no grant.** `w_listen` called `TcpListener::bind` directly,
  with no `network = true` and no `--net`. It was the one capability in the
  harness that was not a capability. The component path denies sockets by
  default.
- **It could not grow.** `wasi:io/poll`'s `poll(list<pollable>)` is a real
  path to serving many connections from one guest loop. `net.*` had no
  equivalent and would have needed new host functions to reach where the
  standard already was.
- **It was Core-only**, so every server was locked out of the component path,
  providers, and the generated boundary.

What actually blocked the retirement was never the transport: `examples/server/`
was a Core app linking `libs/http/http.wasm` as a lib and `libs/sha256.wasm` as
a bridge, and the component text format cannot embed a prebuilt `.wasm`. Both
halves dissolved rather than needing the composition work:

- The HTTP helpers were *source*, so they became `examples/server/src/http.wat`
  and are `;; @include`d. They also changed shape: the Core lib wrote pieces to
  a socket, and a component holds an `output-stream` handle, which cannot cross
  a provider boundary. Buffering the whole response and writing once is both
  simpler and fewer crossings. `libs/http/` is deleted.
- The digest was already a component. `[[bridges]]` with its four offset fields
  became one `[[providers]]` line pointing at the same vendored
  `ai-direct:sha256` package `examples/sha256sum/` consumes -- the first time
  one catalog package serves two applications.

The remaining pure helpers, `$parse_request` and `$mime_for`, are the natural
next catalog provider precisely because they touch no handles: bytes in, values
out. That is not done, and it is not urgent.

Converting it surfaced three `air dist` bugs that no example had been in a
position to find. A distribution copied `[[libs]]` and `[[bridges]]` but not
`[[providers]]`, and it dropped `network = true`: both made a packaged server
fail at runtime rather than at packaging time -- missing provider, or
`access-denied` on the first socket call. The third was older and unrelated to
components: a `root` that cannot be copied into the bundle -- `"../.."` in
`examples/sha256sum/`, or `"."` anywhere -- failed with "root has no directory
name", because the path was never resolved before its file name was taken. A
grant like that is a development convenience, not a thing to ship, so dist now
resolves the path, packages the directory when it is one the bundle can
contain, and otherwise omits the grant with a note saying the packaged app
needs `--dir`. Omitting narrows what the app can reach, which is the safe
direction. All three are fixed, and `air/tests/cli.rs` packages both examples
and runs each copy on its own.

Deleted with it: `air/src/net.rs`, the socket table in `Host`, `Mode::Server`,
`workers` and the host-owned worker pool in `cmds/run.rs`, the `i32 -> i32`
server entry signature in `cmds/check.rs`, and `examples/server/mt.toml`. A
manifest that still says `mode = "server"` now fails to parse, which is the
right way to find out.

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

`ui.*` has not followed, for a reason that is not about WASI: its Core
signatures pass pointers into guest memory, which has no meaning across a
component boundary. It needs value-based signatures (`string`, `list<u8>`)
first. That is a redesign, not a blocker.

`net.*` did not need the trip at all, and it has been deleted. It existed
because Core WASM had no sockets; a component asks for `;; @wasi sockets` and
gets `wasi:sockets` straight from the WIT. See Retiring net.*.

### What Actually Needs A Harness Change

`;; @wasi` names a handful of interfaces, so it is fair to ask whether every new
capability means new harness code -- which would make `air` a bottleneck rather
than a runtime. It does not. Three categories, and only one of them is the
harness's:

**Interfaces WASI already defines: no harness change.** `air` links the whole
WASI 0.2 set through `p2::add_to_linker_sync` -- cli, io, filesystem, sockets,
clocks, random. An application reaches any of them by declaring the import in
its own WAT; `examples/sha256sum/` did exactly that with 51 hand-written
`wasi:filesystem` lines, including the 37-case `error-code` enum, before the
directive learned to generate them. The `;; @wasi` directive is a shorthand
for the boundary most programs need, never a gate on the ones they do not.

**Host policy: genuinely the harness's job.** Whether argv reaches the guest,
which directories are preopened, whether a clock is real or frozen -- WASI
defines the interface but not the answer. Forwarding `air run <manifest>
<args...>` to the guest is a decision, and making decisions about the outside
world is what a runtime is for. Every runtime has this surface; `wasmtime` calls
it `--dir` and argv passing. It is small and it closes.

`air run [--dir <path>...] <manifest> [args...]` is that surface: host options
come first so an application never has to escape its own flags away from the
harness's, and everything from the manifest on belongs to the guest. `root` in
the manifest grants a directory too, resolved relative to the manifest, which is
right for a server's document root and wrong for a tool pointed at an arbitrary
file -- hence the flag.

This is also the sharpest edge a new user meets. WASI has no global filesystem
root: a component reaches only preopened directories, and an absolute path is
not a path to anywhere on its own. A tool that takes a file argument therefore
has to strip a leading `/` and try each grant, as `examples/sha256sum/` does,
and its failure message has to say what was not granted. A sandbox that refuses
without explaining itself reads as a broken program.

**The project's own ABIs.** `ai-direct:host/term`, `[[providers]]`,
`[[bridges]]`. These extend the harness once so every application benefits,
which is the stated point of the repository.

So the shorthand was the only thing that grew per interface, and it no longer
does: `filesystem` and `sockets` are both generated from the interface's own
WIT -- which ships with Wasmtime and is what `examples/sha256sum/`'s enum was
transcribed from -- and narrowed by the application's own imports. Adding
`wasi:clocks` or `wasi:random` is a table entry in `air/src/wit.rs`, not new
emitter code. Hand-writing a rare import remains available, and is a
declaration an author makes once.

Sockets did add one entry to the host-policy category, and it belongs there:
`wasmtime` disables TCP, UDP and name lookup by default, so whether a guest may
open a socket is an answer the runtime has to give. `network = true` in the
manifest and `air run --net` are that answer, next to `[[dirs]]` and `--dir`.

### Granting Directories, And Where An App Keeps State

A component has no ambient filesystem. It reaches exactly what it was granted,
and there are two anchors, deliberately different:

| Grant | Resolved against | For |
| --- | --- | --- |
| `root` / `[[dirs]]` in the manifest | the **manifest** | the application's own directories |
| `--dir` / `--dir-rw` on the command line | the **shell's working directory** | whatever the user is pointing at |

The split is the useful part. A manifest path is project-relative, so it names
the same directory wherever `air` was launched from and it travels with the app
through `air dist` -- next to a distributed binary, project-relative *is* the
install directory. A command-line grant is relative to the user's shell,
because that is where the user is looking.

```toml
[[dirs]]
path = "data"      # project-relative: <manifest dir>/data
write = true       # read-only otherwise; writing is the exception
```

**This is where a stateful app keeps state.** A SQLite database, a cache, a log:
declare one writable `[[dirs]]` entry and the app owns it. `air` creates it on
first run, so an application does not have to ship an empty directory, and a
read-only grant refuses the write rather than silently dropping it.

Nothing is writable unless a grant says so, and `root` stays read-only, so
adding writes to an app is a visible edit to its manifest rather than a
property it acquires quietly.

WASI has no global filesystem root, which is the sharpest edge here: an absolute
path is not a path to anywhere on its own. A tool that takes a file argument
strips a leading `/` and tries each grant, as `examples/sha256sum/` does, so
`--dir /` makes an absolute path behave the way a shell user expects.

The network is the same kind of grant, with one switch rather than a path:

```toml
network = true     # `wasi:sockets` answers; without this, access-denied
```

`air run --net <manifest>` is the shell-side form, for a manifest that does not
ask. One grant covers TCP, UDP and name lookup, because the boundary already
declares nothing the application did not import: `examples/tcp-hello/` imports
nine TCP functions and therefore cannot reach UDP whatever the grant says.

### Consuming Someone Else's Compiled Work

`ai-direct:sha256` is the catalog's first package and the first proof that the
premise reaches past hand-written IR. The cryptography is RustCrypto's `sha2`,
used unmodified; the package adds a WIT contract, a `no_std` adapter, a
reproducible build, and a conformance test against coreutils `sha256sum`.

`wit-bindgen` is not required and was not installed. It generates canonical ABI
glue, and an adapter that exports that shape directly is enough for
`wasm-tools component embed` and `component new` to lift a core module into a
component. The released artifact imports nothing at all -- no WASI, no ambient
authority -- so it is a pure function of its input and trivial to audit.

`examples/sha256sum/` vendors it hash-locked and consumes it through
`[[providers]]`, so the whole chain is proved by execution: upstream crate ->
component -> vendored package -> application -> a digest that matches
`sha256sum` byte for byte.

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

### WASI 0.3, And Why Not Yet

WASI 0.3 (Preview 3) folds async into the Component Model itself: `wasi:io`
disappears and its work moves into the canonical ABI as native `stream<t>`
and `future<t>` types. Stdin stops being a resource with a
`blocking-read` method and becomes
`read-via-stream: func() -> tuple<stream<u8>, future<result<_, error-code>>>`.
That is the right long-term shape, and it is not adoptable yet.

What the pinned toolchain actually offers, as of `wasmtime 48.0.1`:

- `wasmtime-wasi` gates p3 behind a non-default `p3` feature; the defaults are
  `p1` and `p2`. The module documents itself as an "experimental, unstable and
  incomplete implementation ... not ready for production use", and states that
  p3-only bug and security fixes will not get patch releases.
- `wasmtime`'s own `Config::wasm_component_model_async` says support for the
  proposal is "very incomplete".
- p3 covers `cli`, `clocks`, `filesystem`, `random`, and `sockets`. There is no
  p3 `wasi:http`.
- p3 ships no `add_to_linker_sync`. It is async by construction, while `air` is
  synchronous throughout (`p2::add_to_linker_sync` and `Linker::instantiate` in
  `component.rs`, `p1::add_to_linker_sync` in `link.rs`). Adopting p3 means an
  async runtime inside the harness and async plumbing through `cmds/`,
  `net.rs`, and the eframe path.

By contrast `p2` is a default feature, its implementation carries no such
caveat, and its WIT is at `0.2.12` — a maintained patch line on a released
standard, extended additively through `@since` gates. That is the base to build
on.

The interesting part is that this is a harness question, not an application
one. `;; @wasi` already hides the WASI version from applications: `sha256sum`
imports `"wasi" "read"` and `"wasi" "write"` from the generated `$wasi` core
instance, and those names are the harness's ABI, not WASI's. If `boundary.rs`
can wrap `stream.read` in a blocking shim that keeps those core signatures,
a p3 move costs the generator and nothing else — no example changes. Whether a
synchronously-lifted core task can block on `stream.read` in Wasmtime is the
one question a spike would settle, and it is unanswered here. Note also that
`examples/server/` and every `native` target are Preview 1 and unaffected
either way.

The text format is already ready: `wat 1.258` / `wast 258` parse `stream` and
`future` types along with the `stream.read`, `future.read`, `task.return`, and
`waitable-set.wait` intrinsics, and `futures` and `bytes` are already in
`air/Cargo.lock`. A spike needs no new dependency download.

Revisit when any of these fires: `wasmtime-wasi` drops the "not ready for
production use" language from p3, p3 becomes a default feature, or a p3
`wasi:http` lands that a real application here wants. Until then p3 is a watch
item, not a milestone — and a harness that carried both p2 and p3 at once would
be exactly the compatibility layer this project declines to build without a
real consumer.

## Next Work

Ordered so that each step is provable on its own, and so the catalog stops
being specification-only before more specification is written.

1. Extend the WIT-driven boundary to the remaining interfaces and provider
    contracts. `filesystem` and `sockets` are generated from the vendored WASI
    WIT; `wasi:clocks`, `wasi:random` and the catalog's provider WIT still go
    through hand-written declarations. The granularity rule (the application's
    own imports name what to generate), the naming rule (the WIT export key,
    minus its bracketed kind) and handle release (`<resource>.drop`) are all
    settled, so a new WASI interface is now a table entry in `air/src/wit.rs`
    and nothing else. What is left is not WASI: a provider's WIT arrives as a
    file rather than a vendored constant, and the emitter has never been
    pointed at one. See The Boundary From WIT and What Actually Needs A Harness
    Change.
2. Continue up the memory ladder: records with named fields, then a real
   allocator. Segment addresses are handled (see Harness-Placed Segments) and
   a request loop can now release a whole iteration at once (see Releasing
   Memory), which is as far as a bump pointer goes. Freeing in arbitrary order,
   and growing past one page, still wait on an application that states the
   requirement — a long-lived collection rather than a per-request buffer.
   Grow the mail example far enough to state it.
3. Give `ui.*` value-based signatures so components can import it, then
   convert `gui-hello`. `term.*` already made the trip as
   `ai-direct:host/term`; `net.*` needed no trip and has been deleted.
4. Decide build-time composition only when a released provider needs a single
   fused artifact or handle passing. See Provider Linking above. Converting
   `examples/server/` did not need it: source libs `;; @include`, and a
   compiled one was already a provider component.
5. Add a separate component consumer proof in the mail example. Do not force
   the existing Core WAT app to call a WIT component without an explicit
   component boundary.
6. After the component path works end to end, present SQLite candidates for
   approval; only then add a generic writable data capability and a
   `mail-store` provider.
7. Keep a standing check on WASI 0.3 rather than scheduling it. See WASI 0.3,
   And Why Not Yet for the state of the implementation and the conditions that
   would turn it into real work.

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
