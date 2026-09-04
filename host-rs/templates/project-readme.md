# __APPNAME__

`__APPNAME__` is a __TARGET_NAME__ WebAssembly application. Its source is
readable WebAssembly Text (`.wat`), configured by `host.toml`.

This fresh scaffold is a small, working starting-point example. Change it to
build the requested application; its starter behavior and layout are not a
product requirement.

## Develop

Prerequisites: `host-rs` and `wat2wasm` (from wabt) must be on `PATH`.

```bash
host-rs build
host-rs check
__RUN_COMMAND__
```

`build` writes the `[app].path` artifact from `[app].source`. `check` validates
the compiled module without running it. Run both after every WAT or manifest
change.

__TARGET_WORKFLOW__

## Files

| File | Purpose |
|---|---|
| `__APPNAME__.wat` | Application source. Keep its memory map and ABI assumptions documented. |
| `__APPNAME__.wasm` | Generated artifact. Rebuild; do not edit it directly. |
| `host.toml` | Target, entry point, and application configuration. |
__TARGET_FILES__
| `AGENTS.md` | Implementation and verification rules for this application. |

## Application Contract

__TARGET_CONTRACT__

## Changing The App

Start with the behavior requested for this application. Keep state, layout,
and policy in WAT. If a capability is missing, describe the required ABI and
its security boundary before changing the target host. Do not add arbitrary
host-language execution as an application shortcut.

Read `AGENTS.md` before editing. It defines the target-specific rules and the
required verification workflow.
