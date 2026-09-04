# Terminal capability (`term.*`)

WASI preview1 gives a guest stdin/stdout byte stream. That is sufficient for
line prompts but deliberately does **not** define raw mode, key events, cursor
movement, screen clearing, terminal dimensions, or alternate-screen cleanup.
`host-rs` provides those as an explicit, optional terminal capability backed by
the portable Rust `crossterm` crate.

An app receives no terminal privilege merely by being run through the harness:
refuses when stdin or stdout is redirected. Guests must retain a scripted
fallback for pipes and CI, as `examples/prompts/` does.

## ABI v1

All functions return `i32`; 0 means success and -1 means unavailable/error
unless noted otherwise.

| Import | Signature | Meaning |
|---|---|---|
| `term.available` | `()->i32` | 1 if stdin and stdout are terminals, else 0. |
| `term.enter` | `()->i32` | Enables raw mode, enters alternate screen, hides cursor, clears it. Idempotent. |
| `term.exit` | `()->i32` | Shows cursor, leaves alternate screen, restores terminal mode. Idempotent. |
| `term.clear` | `()->i32` | Moves to 0,0 and clears alternate screen. Requires `enter`. |
| `term.move_to` | `(x:i32,y:i32)->i32` | Moves cursor; coordinates must fit u16. Requires `enter`. |
| `term.size` | `()->i32` | `(columns << 16) | rows`, or -1. |
| `term.flush` | `()->i32` | Flushes host stdout. |
| `term.read_key` | `()->i32` | Blocks for key press. Printable ASCII returns its byte. Special values below. |

`read_key` special values: Up `0x101`, Down `0x102`, Left `0x103`, Right
`0x104`, Backspace `0x108`, Tab `0x109`, Enter `0x10d`, Escape `0x11b`, Ctrl-C
`3`. It ignores release/repeat events so one physical key press is one guest
event.

## Cleanup invariant

The harness records terminal activation in its per-instance `Host` state.
`Host::Drop` calls `term::restore`, which disables raw mode and restores the
normal screen/cursor even if the WASM guest traps or returns an error. Apps
should still call `term.exit` before printing their final result so output is
visible in the normal shell.

## Use

```bash
wat2wasm examples/prompts-raw/prompts-raw.wat -o examples/prompts-raw/prompts-raw.wasm
host-rs check examples/prompts-raw/prompts-raw.toml
host-rs examples/prompts-raw/prompts-raw.toml
```

Use Up/Down, Space, Enter, Escape, or Ctrl-C. `examples/prompts-raw/` provides
a responsive environment select, feature checkbox multiselect, confirmation,
cancellation, and summary. It uses the `libs/text-width/` Rust bridge for
Unicode display-cell width, so its title is centered correctly despite ANSI
escapes and Unicode byte counts. `examples/prompts/` remains the line-based,
pipeable setup flow. Build richer widgets from these primitives only when they
are an actual app need, rather than adding a bespoke host function per widget.
