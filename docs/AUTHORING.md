# Authoring WAT Applications on `air`

How an AI writes application behavior in WebAssembly Text (WAT) for the
`air` harness. Every example in `examples/` follows these rules;
`examples/hello/hello.wat` is the smallest complete program.

## Component shape

Every app is a WASI 0.2 component. Source is a single `(component ...)`
WAT root; `target = "component"` is the default and is inferred from the
artifact, so the manifest rarely names it. `browser` (Core WASM against a
generated Canvas host) is the only other target. There is no `target =
"gui"`: a GUI app is an ordinary component with `mode = "gui"` that
imports `ai-direct:host/ui`.

Start every source file with a memory-map header comment:

```wat
;; Memory map (1 page): 0x100 message, 0x200 stream result,
;;                      0x8000+ canonical ABI bump allocation
```

## The generated boundary: `;; @wasi`

Never hand-write the WASI 0.2 boundary. One directive inside
`(component ...)` generates the imports, shared memory, and canonical
ABI lowering:

```wat
(component
  ;; @wasi stdin stdout stderr exit pages=2 heap=0x8000
```

Capabilities: `stdin`, `stdout`, `stderr`, `exit`, `exit-with-code`,
`args`, `filesystem`, `sockets`, `term`, `ui`. `pages=` (default 1)
sizes the memory; `heap=` (default `0x8000`) places the canonical ABI
bump allocator above the application's fixed addresses. An unknown word
is an error; a second directive is rejected.

Generated names the application links against:

| Name | What it is |
| --- | --- |
| `$mem` | core instance exporting `memory`, plus `heap-mark` / `heap-reset` on request — `(with "env" (instance $mem))` |
| `$wasi` | core instance of lowered imports — `(with "wasi" (instance $wasi))` |
| `$memory` / `$realloc` | the memory and its bump allocator, for lowering further imports |
| `$fs` | lowered `wasi:filesystem` imports — `(with "fs" (instance $fs))` |
| `$net` | lowered `wasi:sockets` imports — `(with "net" (instance $net))` |

`$wasi` exports one Core function per capability: `get-stdin`, `read`,
`get-stdout`, `get-stderr`, `write`, `get-arguments`, `exit`,
`exit-with-code`.

### Import-driven extent

`filesystem`, `sockets`, `term`, and `ui` are generated from their WIT
(`air/wit/wasi-0.2.12/`, plus `air/wit/ai-direct-host/host.wit` for the
harness's own interfaces) and narrowed to what the application imports
from `"fs"`, `"net"`, `"term"`, `"ui"`. An unknown name fails the
build, as does a capability with no import at all.

An import name is the WIT export key minus its bracketed kind:
`descriptor.open-at`, `tcp-socket.subscribe`, `get-directories`.

### Interfaces the directive does not name

The directive is shorthand, never a gate. An interface `;; @wasi` does
not cover is still available: declare the import by hand in WAT. A new
*known* interface is a table entry in `air/src/wit.rs`, never a
transcribed type graph. The harness's own interfaces live in
`air/wit/ai-direct-host/host.wit` — the one WIT file here that is not
vendored, and the contract `component.rs` implements. Change both sides
in that one file.

## Source splitting: `;; @include`

One application compiles to one component. The root WAT owns the outer
`(component ...)`, imports, memory, exports, and source order, and may
pull in fragments with a standalone line:

```wat
;; @include src/views/inbox.wat
```

Paths resolve against the *root* source's directory at every depth —
never absolute, never `..`. Fragments may include further fragments; a
cycle is rejected. Fragments must not add another `(component ...)` /
`(core module ...)`.

## Strings: named segments and `;; @data`

Never hand-write a string address or length:

```wat
;; @data 0x1000..0x8000
(data $intro "\u{25c6} prompts demo\n")
(data $ask-name "\u{25c7} Project name? ")
```

Declare `;; @data <start>..<end>` once; leave named segments unplaced
and read `$msg.ptr` / `$msg.len`. A segment stating a literal offset
keeps it; the two may not overlap, and the region must fit its
segments. The length is the decoded byte count, so `\n`, `\1b`,
`\u{25c6}`, and literal multi-byte characters measure correctly; an
escape the harness cannot measure is an error. Unnamed segments are
untouched. Each `(core module ...)` in a component places against its
own memory.

## Handles: `<resource>.drop`

Every resource a boundary declares offers `<resource>.drop`, a
`(canon resource.drop ...)` builtin spelled like a method:

```wat
(import "net" "tcp-socket.drop" (func $drop_socket (param i32)))
(import "wasi" "input-stream.drop" (func $drop_in (param i32)))
```

Stream resources drop from `"wasi"`; a capability's own resources drop
from its instance. Opt-in by import, and dropping something the
boundary never declared fails the build. Servers must drop accepted
sockets and streams per connection — dropping the socket is what closes
the TCP connection.

## Looping servers: `heap-mark` / `heap-reset`

The canonical ABI heap is a bump pointer that frees nothing. A
component that loops (e.g. an accept loop) imports `"env" "heap-mark"` /
`"env" "heap-reset"`, marks at the top of the iteration, resets at the
bottom, or dies with `realloc return: beyond end of memory`:

```wat
(import "env" "heap-mark" (func $heap_mark (result i32)))
(import "env" "heap-reset" (func $heap_reset (param i32)))
```

Opt-in by import; an unknown `"env"` name is a build error. A frame
loop over `ai-direct:host/ui` needs no reset: guest-to-host `string`
parameters never touch the bump heap.

## Small ABI facts

- `exit` takes a `result` — 0 or 1 only, pass/fail. For a status code,
  ask for `exit-with-code` (`u8`). `available`/`enter` answer `bool`,
  `size` answers `tuple<u32, u32>`, `read-key` answers `u32`.
- Canonical ABI discriminants are `u8`: read with `i32.load8_u`, never
  `i32.load`.
- `term` capability (`ai-direct:host/term`): raw-mode terminal —
  `enter`, `exit`, `available`, `clear`, `move-to`, `flush`, `size`,
  `read-key`. The host restores the terminal however the guest left it.
- `ui` capability (`ai-direct:host/ui`): immediate-mode drawing for
  `mode = "gui"` — `label(text)`, `button(text) -> bool`. The guest
  describes a whole frame and returns; a click reports on the frame
  after the one that drew the button.

## Manifests and capability grants

One manifest per app, beside its modules. Paths resolve
manifest-dir-first and travel with `air dist`.

```toml
mode = "command"   # or "gui": open a window, call the entry point per frame
# target = "component"  # default, inferred from the artifact; rarely written
network = true     # wasi:sockets answers; without it, access-denied
root = "www"       # project-relative directory grant
guest = "/repo"    # where the grant appears inside the sandbox (optional)

[[dirs]]
path = "data"      # project-relative: <manifest dir>/data
write = true       # read-only otherwise; writing is the exception

[[providers]]
path = "vendor/.../provider.component.wasm"  # vendored, hash-locked

[app]
source = "app.wat"
path = "app.wasm"
run = "wasi:cli/run"
```

Nothing is reachable unless it asks. Directories: `root` / `[[dirs]]`
in the manifest (resolved against the manifest, travel with `dist`),
`--dir` / `--dir-rw` from the shell (resolved against the shell's
working directory), `write = true` for state. Sockets: `network = true`
or `air run --net`. WASI has no global root, so an absolute path
resolves nowhere by itself — a tool taking a file argument strips a
leading `/` and tries each grant (see `examples/sha256sum/`), and
`--dir /` makes absolute paths behave the way a shell user expects.

A component consumes another component through `[[providers]]`, wired
at link time with no composition tool; the bundle ships app plus
providers, and resource handles do not cross the boundary — plain
values (`string`, `list<u8>`, records) do. A prebuilt Core module is
lifted with `wasm-tools component new` and consumed through
`[[providers]]`, never linked directly. There is no host socket layer —
a server is a component that owns its accept loop (`examples/server/`,
`examples/tcp-hello/`).

Host options come first on the command line so an application never has
to escape its own flags: `air run [--dir <path>...] [--net] <manifest>
[args...]` — everything from the manifest on belongs to the guest.
