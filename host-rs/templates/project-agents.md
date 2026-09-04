# AGENTS.md — rules for coding agents in `__APPNAME__`

This directory is a WASM app scaffolded by `host-rs` (see the harness repo,
`docs/19-harness.md`). AI writes the IR (`.wat`); the selected native or
browser host supplies declared capabilities.

## Hard rules

- **Never install, upgrade, or remove software without explicit user
  consent.** Missing tool? Stop and ask.
- **Verify by execution.** Every claim ends in a run: `host-rs build` must
  assemble, `host-rs check` must pass, CLI output must match.
- **Keep the harness generic.** New app needs go in `host.toml`, never in
  harness code. If a genuinely new shape
  is needed (a syscall, a bridge arity), it gets built into the host
  once so all apps benefit.
- **Generated = ignored.** Track sources (`.wat`, `.toml`, `.md`);
  never commit build output (`*.wasm` here is a local distributable —
  check your repo's ignore policy).

## Layout

- `__APPNAME__.wat` — the app (memory map in the file header comment)
- `host.toml` — manifest: mode, libs/bridges, entry func
- `__APPNAME__.wasm` — assembled artifact (rebuild after every edit)
- `index.html` / `web-host.js` — browser projects only: page and trusted host

## Build / run

```bash
host-rs build                    # assemble [app].source into [app].path
host-rs check                    # link + verify host.toml, do NOT execute
host-rs run                      # run host.toml
host-rs inspect __APPNAME__.wasm # what it needs and offers
```

For a browser project, use `host-rs serve` after `build` and `check`, then open
its localhost URL; `host-rs run` intentionally only runs native projects.

## Conventions

- WAT: memory map in the file header comment; address ranges shared
  with a lib are ABI — document them.
- Command mode: `_start` entry, WASI stdio, `proc_exit` code becomes
  the process exit code. Server mode: `run(port)` owns listen+accept,
  or export `handle(cfd)` and let `workers = N` parallelize.
- Terminal UX: WASI stdio is line-oriented. Import `term.*` only for a
  real terminal: check `term.available`, preserve a pipe/CI fallback,
  and see the harness `docs/21-terminal.md` for raw mode, key events,
  alternate screen, cursor, and size. The harness restores the terminal
  after guest errors/traps.
- Text layout: UTF-8 byte length is not terminal display width (especially
  with ANSI escapes and Unicode). For centered/aligned terminal text, use a
  compatible width library through a bridge; the harness example is
   `libs/text-width/` (`unicode-width`) with `examples/prompts-raw/`.
- Browser ABI: import only `web.*` capabilities declared in `web-host.js`.
  Canvas primitives are deliberate host calls; never add arbitrary JavaScript
  evaluation or browser APIs to WAT without extending and validating the host.
- Commit messages: short imperative summary.
