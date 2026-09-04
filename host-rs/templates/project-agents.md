# AGENTS.md -- `__APPNAME__`

This is a __TARGET_NAME__ WebAssembly application. Work on the application in
this directory; target runtime details below are part of its contract.

The files created by `host-rs new` are a minimal working example, not a fixed
product. Replace the starter behavior, layout, and data as needed for the
requested application while preserving the target runtime contract.

## Project Shape

- `<app>.wat` is executable application policy, state transitions, and
  presentation behavior.
- `src/` holds WAT fragments by responsibility; `src/README.md` defines the
  ordered `;; @include` source composition convention.
- `host.toml` is executable composition: target, entry point, and only locally
  available provider artifacts.
- `docs/01-spec.md` is the requested behavior and acceptance criteria.
- `docs/02-architecture.md` is state ownership, provider/capability choices,
  sensitive data, and trust boundaries.
- `docs/03-verification.md` is the commands and observable checks proving the
  implementation.
- `.agents/skills/ai-direct-ir/SKILL.md` is the generic project-local workflow
  for an AI working with WAT, WASM, WIT, and providers.

Update the relevant document before or with every non-trivial change. Keep
secrets, generated output, and local provider caches out of source control.

## Rules

- Never install, upgrade, or remove software without explicit user consent.
- Change source, not `__APPNAME__.wasm`; `host-rs check`, the run command, and
  `host-rs dist` rebuild automatically after WAT edits. Use `host-rs build` to
  force a rebuild.
- Run `host-rs check` before claiming an integration works.
- __VERIFY_ACTION__ before claiming user-visible behavior works.
- Keep `docs/01-spec.md` current with requested behavior and acceptance
  criteria. Record state/provider/capability decisions in
  `docs/02-architecture.md`; add executable and manual checks to
  `docs/03-verification.md` before claiming a feature is complete.
- Keep the WAT memory map current. Pointer ranges, byte lengths, and shared
  memory ownership are ABI, not incidental implementation details.
- Read `docs/PROJECT.md` in the harness repository before changing a
  built-in host import. Add application dependencies as declared `[[libs]]` or
  `[[bridges]]`; their namespaces and exports are project-owned. `host-rs
  check` proves the complete declared module graph links.
- Keep generated `.wasm` out of source control unless this application
  deliberately distributes it.
- `.gitignore` excludes generated build output and the future `dist/` release
  bundle. Keep distributable artifacts there rather than beside source files.

## Workflow

```bash
host-rs build
host-rs check
__RUN_COMMAND__
host-rs dist
```

`dist` builds declared WAT source and checks the result before packaging. Run
`build` and `check` separately during development for faster feedback.

__TARGET_AGENT_CONTRACT__

## Scope

Prefer the smallest application-local change. Do not change the harness merely
because this app needs a library: declare a WASM provider in `host.toml` and
import its exports. Built-in host capabilities need an explicit ABI,
implementation, validation, and documentation. The current ABI is experimental:
redesign an inadequate built-in import directly rather than adding compatibility
layers, unless this project has a concrete released dependency on it.

## Version Control

Never commit or push without an explicit request. Finishing a unit of work is
not a request. Leave changes in the working tree, report what changed, and let
the user decide when it lands.
