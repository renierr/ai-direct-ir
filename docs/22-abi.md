# Host ABI contract

`host-rs` is a capability host. WAT modules do not link directly to Rust
crates, browser APIs, or OS libraries. They import this ABI; the selected host
target implements and validates those imports. This document is the normative
contract for every `host-rs` release and every generated project.

## Versioning and compatibility

The ABI is currently **v1**. A module identifies its target with `target` in
`host.toml`; its imports identify the v1 capability namespace (`ui`, `web`,
`term`, `net`, and WASI where allowed).

- Existing import names, parameter/result types, memory representation, and
  documented semantics never change in v1.
- Adding a new import is backward-compatible. It must be implemented,
  validated by `host-rs check`, documented here, and covered by an executable
  example or test before release.
- A breaking change requires a new namespace/version (for example `ui_v2`) or
  a new major harness release with a migration path. Do not repurpose an
  existing import.
- A released capability may be deprecated in this document, but remains
  available for its declared support period. Remove it only in a major ABI
  version.
- The harness version (`host-rs --version`) and the ABI version are separate:
  harness patch/minor releases may add/fix capabilities without breaking v1.

Generated projects should record the minimum harness version they have tested
in their README when they depend on a newly added import.

The native GUI implementation is `egui` through `eframe`, currently linked
into `host-rs`. A GUI distribution contains the harness executable and its WASM
modules, not a separate egui runtime. It still relies on the platform's normal
windowing and graphics drivers (Wayland/X11/OpenGL on Linux; Windows graphics
system on Windows; Metal/OpenGL on macOS). Those are OS prerequisites, not
application dependencies to bundle.

## General guest-memory rules

- `ptr` and `len` are signed 32-bit values. Negative values are invalid.
- Text is UTF-8 stored in the module's declared `env.memory`.
- The host bounds-checks every range before reading it. Invalid ranges or
  invalid UTF-8 fail the host call; they never expose host memory.
- The guest owns its memory and application state. The host owns windows,
  browser objects, operating-system handles, and capability state.
- Calls must not provide arbitrary code, shell commands, JavaScript, native
  pointers, or unrestricted filesystem paths as an escape hatch.

## Target capability matrix

| Capability | `native` | `browser` | `gui` |
|---|---|---|---|
| WASI preview1 | Yes | No | No |
| `term.*` | Yes | No | No |
| `net.*` | Yes | No | No |
| `web.*` | No | Yes | No |
| `ui.*` | No | No | Yes |
| `[[libs]]` | Yes | No | Yes |
| `[[bridges]]` | Yes | No | No |

`host-rs check` is the authority. Imports outside the selected target's table
are rejected before an app is run or distributed.

## GUI ABI v1

`target = "gui"` creates a native immediate-mode desktop application backed
by egui. The configured zero-argument `[app].run` export is called once per UI
frame. The guest emits controls in call order; it keeps all application state
in WASM globals or memory.

GUI projects use `mode = "command"` and a shared `env.memory`. They may link
declared `[[libs]]` modules, allowing AI-authored or independently-produced
WASM libraries to provide pure computation and reusable application behavior.
GUI libraries may import only `env.memory` and documented `ui.*` functions;
they cannot bypass the host with WASI, terminal, network, browser, or bridge
imports. GUI projects may not use WASI, `term.*`, `net.*`, `web.*`, or bridges
in v1.

| Import | WAT signature | Contract |
|---|---|---|
| `ui.label` | `(param i32 i32)` | Render UTF-8 text at `(ptr, len)`. |
| `ui.button` | `(param i32 i32) (result i32)` | Render UTF-8 text at `(ptr, len)`; return `1` when that same labelled button was clicked in the preceding host frame, otherwise `0`. |

Example:

```wat
(import "env" "memory" (memory 1))
(import "ui" "label" (func $label (param i32 i32)))
(import "ui" "button" (func $button (param i32 i32) (result i32)))

(func (export "frame")
  (call $label (i32.const 0) (i32.const 14))
  (if (call $button (i32.const 32) (i32.const 9))
    (then ;; update guest state here
    )))
(data (i32.const 0) "Hello from WAT")
(data (i32.const 32) "Increment")
```

The host deliberately returns button events on the next frame: WAT describes
the current frame before egui receives pointer input for it. This avoids host
callbacks and keeps control flow and state inside the module.

## Libraries and host capabilities

WASM libraries are how applications freely add logic without changing the
harness: math, parsing, layouts, data structures, domain rules, codecs, and
any computation that can operate on WASM memory can be supplied through
`[[libs]]`. The app imports their exports through the manifest's `as`
namespace. The harness auto-wires those exports after validating the target's
allowed imports.

Libraries cannot independently acquire operating-system effects. A module that
needs a window, file picker, filesystem, network, GPU object, clipboard, or
browser object still needs a documented host capability, because only the host
can safely own and permission those resources. This is intentional capability
security, not a WAT limitation. Prefer established implementations behind a
small host adapter rather than rebuilding platform facilities in WAT.

## Browser ABI v1

Browser apps use `target = "browser"`, `mode = "command"`, and only `web.*`.
The implementation is the generated `web-host.js`.

| Import | WAT signature |
|---|---|
| `web.canvas_width`, `web.canvas_height`, `web.mouse_x`, `web.mouse_y` | `() -> i32` |
| `web.key_down` | `(i32) -> i32` |
| `web.clear` | `(i32 i32 i32 i32)` |
| `web.fill_rect` | `(i32 i32 i32 i32 i32 i32 i32 i32)` |
| `web.request_frame` | `()` |

`web.request_frame` requires a zero-argument exported `frame()`.

## Native ABI v1

Native command/server projects retain the existing WASI, `term.*`, `net.*`,
and manifest-declared library/bridge contracts. Their authoritative details are
in `docs/19-harness.md`, `docs/21-terminal.md`, and module manifests. When a
native import is added, add its exact signature and safety contract here too.

## Capability change checklist

1. Define the minimal target-specific import and memory/error contract here.
2. Implement it in the trusted host using an established library where one
   exists; the adapter should stay small.
3. Reject unsupported names and signatures in `host-rs check`.
4. Add a WAT proof project and automated/executable verification.
5. Update generated `AGENTS.md` target guidance and release notes.
6. Keep old v1 behavior intact; version a breaking redesign instead.
