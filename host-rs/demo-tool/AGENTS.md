# AGENTS.md — rules for coding agents in `demo-tool`

This directory is a WASM app hosted by `host-rs` (a generic harness that
links + runs configured WASM modules — see the harness repo,
`docs/19-harness.md`). AI writes the IR (`.wat`); the harness runs it.

## Hard rules

- **Never install, upgrade, or remove software without explicit user
  consent.** Missing tool? Stop and ask.
- **Verify by execution.** Every claim ends in a run: `wat2wasm` must
  assemble, `host-rs check` must pass, CLI output must match.
- **Keep the harness generic.** New app needs go in the manifest
  (`demo-tool.toml`), never in harness code. If a genuinely new shape
  is needed (a syscall, a bridge arity), it gets built into the host
  once so all apps benefit.
- **Generated = ignored.** Track sources (`.wat`, `.toml`, `.md`);
  never commit build output (`*.wasm` here is a local distributable —
  check your repo's ignore policy).

## Layout

- `demo-tool.wat` — the app (memory map in the file header comment)
- `demo-tool.toml` — manifest: mode, libs/bridges, entry func
- `demo-tool.wasm` — assembled artifact (rebuild after every edit)

## Build / run

```bash
wat2wasm demo-tool.wat -o demo-tool.wasm
host-rs check demo-tool.toml   # link + verify, do NOT execute
host-rs demo-tool.toml         # run (shorthand for `run`)
host-rs inspect demo-tool.wasm # what it needs and offers
```

## Conventions

- WAT: memory map in the file header comment; address ranges shared
  with a lib are ABI — document them.
- Command mode: `_start` entry, WASI stdio, `proc_exit` code becomes
  the process exit code. Server mode: `run(port)` owns listen+accept,
  or export `handle(cfd)` and let `workers = N` parallelize.
- Commit messages: short imperative summary.
