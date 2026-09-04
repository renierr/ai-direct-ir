---
name: ai-direct-ir
description: Work safely with AI-Direct IR WAT, WASM, WIT, providers, and host-rs projects.
---

# AI-Direct IR Skill

Use this skill when changing an AI-Direct IR application, a project-local
provider, or its WIT/Component Model integration.

## Environment

- Every project needs only `host-rs` on `PATH`. It assembles and validates both
  Core WAT and component WAT in-process; do not add WABT, `wit-bindgen`, or a
  language toolchain merely to build an application.
- **Prefer `target = "component"` (WASI 0.2) for new work.** Preview 1 (`target
  = "native"`) remains supported and is still required for raw-mode terminals,
  the `net.*` sockets ABI, `ui.*`, and `[[libs]]`/`[[bridges]]` providers.
- `host-rs check`, target run, and `host-rs dist` rebuild a declared root WAT
  source when it or an included fragment is newer than the generated WASM.
  `host-rs build` forces a rebuild.
- `host-rs build` assembles, validates, and compiles before writing. A failed
  build leaves the previous artifact in place; errors name the fragment file and
  line you wrote, not the expanded text. Progress goes to stderr.
- Never hand-write a string's byte length. Name the data segment --
  `(data $msg (i32.const 0x1000) "...")` -- and read `$msg.ptr` / `$msg.len`,
  which host-rs derives from the segment. A named segment needs a literal
  offset and may not overlap another. Unnamed segments are unchanged.
- `wasm-tools` is an optional cross-check, never required to build an app. Use
  `validate`, `component wit`, and `component targets`; do not place it in an
  application distribution or invoke it at runtime. `wasm-tools compose` is
  deprecated upstream — composition of prebuilt components is an open decision.
- Never install, upgrade, or remove tooling without explicit user consent.

## WASI 0.2 Recipes

`target = "component"` is the default for new work. Your own scaffolded
`<name>.wat` is a complete, working example: read it first. The four
constructs below are the ones that are hard to guess.

**Declare an interface you import.** A signature must reference the
*exported* type id (`$sexp`), never the local type it was defined from, or
validation rejects the whole instance with an unhelpful message:

```wat
(import "wasi:io/streams@0.2.12" (instance $streams
  (export "error" (type $ie (eq $error)))
  (export "output-stream" (type $os (sub resource)))
  (type $se (variant (case "last-operation-failed" (own $ie)) (case "closed")))
  (export "stream-error" (type $sexp (eq $se)))
  (export "[method]output-stream.blocking-write-and-flush"
    (func (param "self" (borrow $os)) (param "contents" (list u8))
          (result (result (error $sexp)))))))
```

**Break the memory cycle.** Lowering an import needs the memory; the logic
module needs the lowered imports. Put the memory in its own core module:

```wat
(core module $mem-mod (memory (export "memory") 1) ...)
(core instance $mem (instantiate $mem-mod))
(alias core export $mem "memory" (core memory $memory))
(core func $write-l (canon lower (func $write) (memory $memory) (realloc $realloc)))
```

**Export `cabi_realloc` when the host returns a value to you.** Anything
returning `list<u8>` or `string` is allocated into your memory through it. A
bump allocator is enough when the program never frees; place its base above
every fixed address in the memory map.

**Lift the entry point.** `run: func() -> result` — `0` is exit 0, `1` is a
failed run. For an exit from deep inside the program, where a return cannot
reach, import `wasi:cli/exit` (`exit: func(status: result)`), the direct
replacement for Preview 1's `proc_exit`.

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
  `(component ...)` WAT that host-rs assembles in-process; no bindings
  generator and no language toolchain are involved. Declare the WASI
  interfaces you import, lower them with `canon lower`, write the logic in an
  ordinary `(core module ...)`, and lift the entry with `canon lift`. A
  function signature must reference the *exported* type id, not the local type
  it was defined from, or the whole instance fails validation.
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
2. Run `host-rs check`, then the target's observable manual or automated check.
3. Run `host-rs dist` for distribution-affecting work.

Never commit generated `.wasm`, `dist/`, `.env`, private data, credentials, or
unreviewed provider caches.
