# __APPNAME__

`__APPNAME__` is a WebAssembly app hosted by
[`host-rs`](https://github.com/renierr/ai-direct-ir): a native harness that
links configured WASM modules, grants only declared capabilities, and runs the
app. The app is written as readable WebAssembly Text (`.wat`); its compiled
`.wasm` is portable, while the `host-rs` binary is built per operating system.

## Quick Start

Prerequisites: `host-rs` and `wat2wasm` (from wabt) must be on `PATH`.

```bash
host-rs build
host-rs check
host-rs run
```

`build` assembles the `[app].source` WAT file declared in `host.toml` into
`[app].path`. Run it after every WAT change. `check` loads and links every
configured module but does not execute it. `build`, `check`, and `run` use
`host.toml` automatically when invoked inside this directory.

## Project Files

| File | Purpose |
|---|---|
| `__APPNAME__.wat` | App source: AI-generated, human-reviewable WASM IR. |
| `__APPNAME__.wasm` | Assembled app artifact. Rebuild with `host-rs build` after changing WAT. |
| `host.toml` | Manifest: app mode, module paths, libraries, bridges, entry function. |
| `AGENTS.md` | Rules and workflow for agents modifying this app. |

## Harness Model

The manifest is the integration boundary. An app imports the capabilities and
libraries it needs; `host-rs` validates and wires those imports to host
syscalls or another module's exports. A new application normally requires no
harness rebuild, only its `.wat`, `.wasm`, and TOML manifest.

Start from a foreign or Cargo-produced module with:

```bash
host-rs inspect some-lib.wasm
host-rs init some-app.wasm
```

`inspect` lists imports and exports. `init` writes a non-overwriting manifest
stub based on the app imports. Use `host-rs --help` for every command.

## Adding Capabilities

- **WASI stdio/files:** import `wasi_snapshot_preview1.*`. The manifest may
  preopen one read-only data directory with `root = "www"`.
- **Network server:** import `net.listen`, `net.accept`, `net.recv`,
  `net.send`, and `net.close`; set `mode = "server"`. See the harness
  `examples/server/`.
- **Reusable shared-memory library:** list it under `[[libs]]`; use imported
  `env.memory` and document address ownership as ABI.
- **Finished own-memory library:** list it under `[[bridges]]`; expose its
  allocator and an explicit bridge call shape. See `libs/sha256/`.
- **Interactive terminal:** import `term.*` only after checking
  `term.available`; preserve a plain stdin/stdout fallback for pipes and CI.
  See `docs/21-terminal.md` in the harness repository.

New reusable syscall or bridge shapes belong in `host-rs` once, not in a
specific app. See the harness documentation, especially `docs/19-harness.md`.

## Development Rules

Read `AGENTS.md` before changing the app. In brief: do not install tooling
without approval; keep WAT's memory map documented; use `check` plus an
end-to-end run as verification; and keep generated build output out of source
control unless your project deliberately ships it.
