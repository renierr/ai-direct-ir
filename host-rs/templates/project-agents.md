# AGENTS.md -- `__APPNAME__`

This is a __TARGET_NAME__ WebAssembly application. Work on the application in
this directory; target runtime details below are part of its contract.

The files created by `host-rs new` are a minimal working example, not a fixed
product. Replace the starter behavior, layout, and data as needed for the
requested application while preserving the target runtime contract.

## Rules

- Never install, upgrade, or remove software without explicit user consent.
- Change source, not `__APPNAME__.wasm`; run `host-rs build` after WAT edits.
- Run `host-rs check` before claiming an integration works.
- __VERIFY_ACTION__ before claiming user-visible behavior works.
- Keep the WAT memory map current. Pointer ranges, byte lengths, and shared
  memory ownership are ABI, not incidental implementation details.
- Read `docs/22-abi.md` in the harness repository before adding or changing
  imports. Only documented target capabilities are valid; `host-rs check`
  rejects all others.
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

Prefer the smallest application-local change. A new host capability needs an
explicit ABI, implementation, validation, and documentation; do not invent
undeclared imports or bypass the target host with arbitrary code execution.
ABI additions are backward-compatible only: never change an existing import's
signature, memory contract, or behavior in place.
