# Own versus leverage

The rule is not "write WAT for everything". AI-generated WAT should own the
thin policy layer that makes the app distinctive; mature, compatible libraries
should provide well-specified commodity work.

## The decision checklist

1. Is the dependency a core WASM module, or can it be compiled to one? JS
   packages and Python packages are not — they need their own runtime.
2. Does it fit an existing contract? A shared `env.memory` ABI is zero-copy;
   an own-memory module fits the manifest bridge only if it can expose an
   allocator and a declared bridge call shape.
3. Does it need a capability the harness does not expose? Adding raw terminal
   mode, async sockets, threads, or a new bridge shape is a host product
   decision: extend it once only when the need is real and reusable.
4. Is the remaining code small, auditable, and app-specific? Own it in WAT.
   Is it a large, security-sensitive, standards-heavy algorithm? Leverage it.

## Prompts example

`examples/prompts/` implements text, select, multiselect, validation,
confirmation, cancel, and a final summary in WAT. It deliberately resembles
the *flow* of Clack, not Clack's raw-terminal UX.

Clack itself is an npm package, therefore the wrong VM shape for core WASM.
Rust terminal libraries such as `dialoguer`/`inquire` are also poor fits today:
their value is raw mode, cursor movement, key-by-key input, terminal sizing,
and restore-on-panic. This harness intentionally exposes only WASI stdio, so
there is no termios capability to make those features correct. ANSI colours
are just stdout bytes and would work; instant arrow-key selection would not.

The resulting line-based prompt layer is small enough to own and works both
interactively and with piped answers:

```bash
wat2wasm examples/prompts/prompts.wat -o examples/prompts/prompts.wasm
printf 'demo\n2\n1,3\ny\n' | host-rs examples/prompts/prompts.toml
```

Full terminal UI is now available through the narrow `term.*` ABI
(`docs/21-terminal.md`): raw-mode enter/restore, alternate screen, cursor,
size, and key events. `examples/prompts-raw/` provides responsive select,
checkbox multiselect, confirmation, cancellation, and summary; the ordinary
prompt example retains its pipeable line interface. Its WAT state machine uses
the Cargo `unicode-width` bridge to center styled Unicode text by display cells
rather than UTF-8 byte count. Do not compile a whole JS runtime just to reuse
Clack.

## Scaffold a new app

`host-rs new <name>` creates an empty safe project directory containing
`<name>.wat`, `<name>.toml`, and `AGENTS.md`. The agents template is compiled
into the harness with `include_str!`, so the project remains self-explanatory
when copied outside this repository. The command accepts `[A-Za-z0-9_-]` names
and refuses non-empty or partially existing targets.

```bash
host-rs new hello-ai
cd hello-ai
wat2wasm hello-ai.wat -o hello-ai.wasm
host-rs check hello-ai.toml
host-rs hello-ai.toml
```

The scaffold starts command-mode/WASI-stdio because it is the smallest useful
module. `host-rs inspect`, the generated `AGENTS.md`, and `docs/19-harness.md`
show the upgrade path to shared libs, bridges, servers, and workers.
