---
name: ai-direct-ir
description: Work safely with AI-Direct IR WAT, WASM, WIT, providers, and host-rs projects.
---

# AI-Direct IR Skill

Use this skill when changing an AI-Direct IR application, a project-local
provider, or its WIT/Component Model integration.

## Environment

- Core WAT projects require `host-rs` on `PATH`. It assembles and validates WAT
  in-process; do not add WABT merely to build an application.
- `host-rs check`, target run, and `host-rs dist` rebuild a declared root WAT
  source when it or an included fragment is newer than the generated WASM.
  `host-rs build` forces a rebuild.
- `wasm-tools` is a build-time prerequisite only for Component Model work. Use
  `validate`, `component wit`, `component targets`, and `compose`; do not place
  it in an application distribution or invoke it at runtime.
- Never install, upgrade, or remove tooling without explicit user consent.

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
- For WIT/Component Model work, define a small versioned WIT contract first;
  validate it, verify the provider component against its declared world, then
  compose explicit local artifacts. Core WAT cannot directly import a WIT
  component without an explicit canonical-ABI/component boundary.
- Record provider source/provenance, license, hash, contract, permissions, and
  conformance checks before consuming a released artifact.

## Verification

1. Update specification for behavior, architecture for state/capability/trust
   decisions, and verification for proof before claiming a feature complete.
2. Run `host-rs check`, then the target's observable manual or automated check.
3. Run `host-rs dist` for distribution-affecting work.

Never commit generated `.wasm`, `dist/`, `.env`, private data, credentials, or
unreviewed provider caches.
