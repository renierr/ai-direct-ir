---
name: ai-direct-ir
description: Work safely with AI-Direct IR WAT, WASM, WIT, providers, and AIR projects.
---

# AI-Direct IR Skill

Use this skill when changing an AI-Direct IR application, a project-local
provider, or its WIT/Component Model integration.

## Environment

- Every project needs only `air` on `PATH`. It assembles and validates both
  Core WAT and component WAT in-process; do not add WABT, `wit-bindgen`, or a
  language toolchain merely to build an application.
- **Prefer `target = "component"` (WASI 0.2) for new work.** Preview 1 (`target
  = "native"`) remains supported and is still required for raw-mode terminals,
  the `net.*` sockets ABI, `ui.*`, and `[[libs]]`/`[[bridges]]` providers.
- `air check`, target run, and `air dist` rebuild a declared root WAT
  source when it or an included fragment is newer than the generated WASM.
  `air build` forces a rebuild.
- `air build` assembles, validates, and compiles before writing. A failed
  build leaves the previous artifact in place; errors name the fragment file and
  line you wrote, not the expanded text. Progress goes to stderr.
- Never hand-write a string's byte length or its address. Name the data segment
  and read `$msg.ptr` / `$msg.len`, which `air` derives. Declare `;; @data
  0x1000..0x8000` once, then leave segments unplaced -- `(data $msg "...")` --
  and `air` packs them in source order. A segment that states a literal offset
  keeps it, which is how scratch space and lib ABI maps stay author-owned;
  placed and unplaced segments may not overlap. Unnamed segments are unchanged.
- `wasm-tools` is an optional cross-check, never required to build an app. Use
  `validate`, `component wit`, and `component targets`; do not place it in an
  application distribution or invoke it at runtime. `wasm-tools compose` is
  deprecated upstream — composition of prebuilt components is an open decision.
- Never install, upgrade, or remove tooling without explicit user consent.

## WASI 0.2 Recipes

`target = "component"` is the default for new work. Your own scaffolded
`<name>.wat` is a complete, working example: read it first.

**Ask for the boundary; do not write it.** One directive inside `(component`
generates the interface imports, the shared memory and the canonical ABI
lowering:

```wat
(component
  ;; @wasi stdin stdout stderr exit pages=2 heap=0x8000
```

Capabilities are `stdin`, `stdout`, `stderr`, `exit`, `exit-with-code`;
`pages=` (default 1) and
`heap=` (default `0x8000`, the canonical ABI bump base) are optional. An
unknown word is an error, not a silent omission. The generated names are the
boundary ABI:

| Name | What it is |
| --- | --- |
| `$mem` | core instance exporting `memory`; instantiate with `(with "env" (instance $mem))` |
| `$wasi` | core instance of lowered imports; `(with "wasi" (instance $wasi))` |
| `$memory` / `$realloc` | the memory and its bump allocator, for lowering your own imports |

`$wasi` exports one Core function per capability: `get-stdin`, `read`,
`get-stdout`, `get-stderr`, `write`, `exit`. Write the program against those in
an ordinary `(core module $main ...)`.

**Lift the entry point.** `run: func() -> result` — `0` is exit 0, `1` is a
failed run:

```wat
  (core instance $app (instantiate $main
    (with "env" (instance $mem))
    (with "wasi" (instance $wasi))))
  (func $run (result (result)) (canon lift (core func $app "run")))
  (instance $run-i (export "run" (func $run)))
  (export "wasi:cli/run@0.2.12" (instance $run-i))
)
```

For an exit from deep inside the program, where a return cannot reach, add an
exit capability. `exit` takes a `result` -- 0 or 1, nothing else -- so passing
it a status code traps on an unexpected discriminant. Use `exit-with-code`
(`u8`) when the program has real exit codes; it is the direct replacement for
Preview 1's `proc_exit`.

**Consume a provider.** Declare it in the manifest and import its interface
like any other:

```toml
[[providers]]
source = "provider.wat"
path = "provider.wasm"
```

`air` instantiates the provider and forwards its exports into your imports.
Plain values cross freely; resource handles do not, because each component
instance owns its own table. The project's own capabilities arrive the same
way: `ai-direct:host/term` is the terminal, imported exactly like a WASI
interface.

Read the WIT for an interface before declaring it. It ships with Wasmtime:
`wasmtime-wasi-*/src/p2/wit/deps/{cli,io,filesystem,sockets}.wit`.

## Core WAT

1. Read `AGENTS.md`, `docs/01-spec.md`, `docs/02-architecture.md`,
   `docs/03-verification.md`, and `src/README.md` before changing behavior.
2. Keep the root WAT file as the module boundary and ordered source index.
   Place product-owned WAT in the smallest `src/` fragment by responsibility:
   state, input, domain policy, views, strings, or provider adapter.
3. Add ordered `;; @include relative/path.wat` lines for fragments. They are
   project-local text within one module and must not contain `(module ...)`.
4. Treat pointers, byte lengths, memory ownership, imports, and exports as ABI.
   Keep the root memory map/documentation current with every raw-memory change.

## Providers And WIT

- Keep product policy, state transitions, and presentation in the application.
  Use a declared project-local provider for mature implementations such as
  databases, protocol stacks, codecs, cryptography, and platform integration.
- A separate Core WASM module is a provider with an explicit manifest ABI, not
  a source-file organization technique. Do not modify the harness merely
  because one application needs a library.
- `target = "component"` runs a WASM component on WASI 0.2. Its source is a
  `(component ...)` WAT that `air` assembles in-process; no bindings
  generator and no language toolchain are involved. Ask for the WASI boundary
  with `;; @wasi <capabilities>`, write the logic in an ordinary
  `(core module ...)`, and lift the entry with `canon lift`.
- A component app cannot declare `[[libs]]` or `[[bridges]]`: those are Core
  WASM mechanisms and mean nothing across a component boundary.
- For WIT/Component Model work, define a small versioned WIT contract first;
  validate it, then verify the provider component against its declared world.
  A WIT directory is one package, so each contract needs its own directory.
  Core WAT cannot directly import a WIT component without an explicit
  canonical-ABI/component boundary.
- Record provider source/provenance, license, hash, contract, permissions, and
  conformance checks before consuming a released artifact.

## Verification

1. Update specification for behavior, architecture for state/capability/trust
   decisions, and verification for proof before claiming a feature complete.
2. Run `air check`, then the target's observable manual or automated check.
3. Run `air dist` for distribution-affecting work.

Never commit generated `.wasm`, `dist/`, `.env`, private data, credentials, or
unreviewed provider caches.
