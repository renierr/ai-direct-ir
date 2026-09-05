# __APPNAME__

`__APPNAME__` is a __TARGET_NAME__ WebAssembly application. Its source is
readable WebAssembly Text (`.wat`), configured by `host.toml`.

This fresh scaffold is a small, working starting-point example. Change it to
build the requested application; its starter behavior and layout are not a
product requirement.

## AI-Agent Project Shape

This project keeps different kinds of context in different files so an agent can
change it safely: WAT contains executable application behavior, `host.toml`
contains executable module composition, and `docs/` records intent, design
decisions, and proof. Keep those three views synchronized; do not hide product
requirements only in source comments or dependency details only in prose.

## Develop

Prerequisite: `air` must be on `PATH`. It assembles and validates WAT
in-process; no separate WAT compiler is required.

```bash
air build
air check
__RUN_COMMAND__
air dist
```

`build` always writes the `[app].path` artifact from `[app].source`. `check`,
the run command, and `dist` rebuild automatically when source is newer than
the artifact or it is missing. `check` validates without running the app;
`dist` replaces the ignored `dist/` directory with a self-contained bundle.

__TARGET_WORKFLOW__

## Files

| File | Purpose |
|---|---|
| `__APPNAME__.wat` | Application source. Keep its memory map and ABI assumptions documented. |
| `__APPNAME__.wasm` | Generated artifact. Rebuild; do not edit it directly. |
| `host.toml` | Target, entry point, and application configuration. |
__TARGET_FILES__
| `src/` | Modular WAT fragments and their source-layout guide. |
| `docs/01-spec.md` | User-visible behavior, non-goals, and acceptance criteria. |
| `docs/02-architecture.md` | State, module/provider boundaries, and capability decisions. |
| `docs/03-verification.md` | Executable checks and manual behavior to prove. |
| `.gitignore` | Excludes generated artifacts, local build output, and `dist/` release bundles. |
| `AGENTS.md` | Implementation and verification rules for this application. |
| `.agents/skills/ai-direct-ir/SKILL.md` | Generic AI workflow for WASM/WIT work in this project. |

## Application Contract

__TARGET_CONTRACT__

## Changing The App

Start with the behavior requested for this application. Keep state, layout,
and policy in WAT. Add reusable behavior by declaring WASM providers in
`host.toml`; do not change the harness merely because this app needs a library.
Only propose a new built-in host capability after a project-owned provider or
available WASI capability cannot solve the requirement. This is a builder-phase
project: current host interfaces may be redesigned directly rather than carried
forward through compatibility layers.

Read `AGENTS.md`, `src/README.md`, and `.agents/skills/ai-direct-ir/SKILL.md`
before editing. They define the target-specific rules, source organization,
generic WASM/WIT workflow, and required verification. Read and update the
relevant `docs/` file before implementing a non-trivial behavior change.
