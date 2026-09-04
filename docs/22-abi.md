# Experimental Host Interfaces

`host-rs` loads, composes, validates, and packages AI-authored Core WASM
applications. It is not the catalog of application libraries. A project can
declare any number of WASM providers in its manifest and import their exports
under project-owned namespaces. Built-in host imports are only the small set of
effects that need a native or browser implementation.

This document records the current experimental interfaces. A project-owned
provider's module exports are its contract; `host-rs check` proves that the
complete declared graph links before an app runs or ships.

## Builder-phase policy

There are no users and no released compatibility promise. Change, remove, or
replace the current manifest shape, Core WASM linker, `ui.*`/`web.*` imports,
or generated templates whenever the design becomes clearer. Do not add shims,
aliases, versioned namespaces, migration code, or compatibility layers merely
to preserve builder-phase experiments.

The goal is a typed WIT/Component Model provider boundary. Current `[[libs]]`,
`[[bridges]]`, and built-in import namespaces are proofs and transitional
tools, not the final public application format. Preserve a previous shape only
when it has a concrete active consumer or an explicit release commitment.

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
- The host owns windows, browser objects, operating-system handles, and the
  permissions it grants to WASI or built-in host calls.

## Composition model

`[[libs]]` and `[[bridges]]` are project-owned providers, not harness features
to extend per library. A provider may export any Core WASM function, memory,
table, or global under its declared `as` namespace. Applications and providers
may themselves import other declared providers, WASI, or a documented built-in
host capability. The linker resolves the graph and fails on every missing name
or incompatible type.

Use a `[[libs]]` provider when its memory can be shared with the app. Use a
`[[bridges]]` provider when it owns memory and matches the documented copying
call shape. This lets an AI add pure computation, codecs, parsers, databases
compiled to WASM, protocol stacks, and any other WASM library without changing
`host-rs`.

The Core WASM model cannot directly load an arbitrary `.so`, `.dll`, Python
package, Java JAR, or system SDK. Such a dependency needs a provider adapter:
a WASM build of the library where possible, or a future project-shipped native
plugin/sidecar provider. That is an application packaging concern, not a reason
to add library-specific imports to the harness. The planned Component Model
target will use WIT/WASI interfaces for typed provider composition.

## Built-in host capabilities

| Capability | `native` | `browser` | `gui` |
|---|---|---|---|
| WASI preview1 | Yes | No | Yes |
| `term.*` | Yes | No | Yes |
| `net.*` | Yes | No | Yes |
| `web.*` | No | Yes | No |
| `ui.*` | No | No | Yes |
| `[[libs]]` | Yes | No | Yes |
| `[[bridges]]` | Yes | No | Yes |

The table identifies built-in host implementations, not an import allowlist for
native or GUI projects. `host-rs check` validates by linking declared modules.
Browser projects remain tied to their generated browser host until browser-side
provider composition is implemented.

## Experimental GUI Imports

`target = "gui"` creates a native immediate-mode desktop application backed
by egui. The configured zero-argument `[app].run` export is called once per UI
frame. The guest emits controls in call order; it keeps all application state
in WASM globals or memory.

GUI projects use `mode = "command"`. `ui.*` is a built-in egui convenience,
not the GUI application's dependency boundary. GUI apps can declare and import
the same `[[libs]]` and `[[bridges]]` providers as native apps. The common
linker also supplies its built-in WASI, `term.*`, and `net.*` capabilities when
a module imports them. Use only dependencies the project declares and ships.

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

## Providers and host capabilities

WASM providers are how applications freely add behavior without changing the
harness. The app imports their exports through the manifest's `as` namespace;
the export list and WASM type signatures are the contract. A provider may use
other providers and available built-in effects, so it is not limited to pure
calculation. Keep effects explicit in `host.toml` and bundle every provider
needed by the app.

## Experimental Browser Imports

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

## Experimental Native Imports

Native command/server projects retain the existing WASI, `term.*`, `net.*`,
and manifest-declared library/bridge contracts. Their authoritative details are
in `docs/19-harness.md`, `docs/21-terminal.md`, and module manifests. When a
native import is added, add its exact signature and safety contract here too.

## Change Checklist

1. Prefer a WIT-described project-owned provider over a new host API.
2. Replace an inadequate experimental shape directly; do not preserve it by
   default.
3. Implement a built-in host effect only when it cannot live in a provider.
4. Add a WAT/component proof and executable verification.
5. Update the architecture and generated-project documentation in the same
   change.
