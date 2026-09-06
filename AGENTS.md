# AGENTS.md — rules for coding agents in this repo

`air` is the product: a generic harness that links and hosts configured WASM
apps. Examples, libs and docs exist to prove it. AI writes the IR (`.wat`);
the harness runs it.

This is the generic platform repository. Library adapters belong in the
sibling `ai-direct-ir-providers` catalog, application behaviour in an example
repo such as `ai-direct-ir-example-mail`. Examples may break while the generic
design changes.

`docs/PROJECT.md` is the living state: implemented behaviour, current gaps,
ordered next work, and the reasoning behind every rule below. Read the section
a task touches, not the whole file. Update it with every host capability
change. `docs/AUTHORING.md` is the WAT/harness syntax reference — how to
write components, `;; @wasi`, `;; @include`, `;; @data`, manifests, and
grants. Read it before authoring or reviewing WAT.

## Hard rules

- **Never install, upgrade or remove software without explicit user consent.**
  Missing tool? Stop and ask.
- **Verify by execution.** Every claim ends in a run: `cargo test`, `air
  check`, real CLI or `curl` output. Raw bytes over pretty output. A behaviour
  worth claiming is worth a test in `air/tests/cli.rs`.
- **Keep the harness generic.** New app needs go in the manifest, never in
  `air` code. New ABI shapes extend the host once, for every app.
- **Builder phase: redesign freely.** Replace experimental interfaces instead
  of layering on them. No compatibility shims without a real consumer.
- **Generated = ignored.** Track `.wat`, `.rs`, `.toml`, `.md`; never
  `target/`, `*.o`, or lib `*.wasm`. Exception: `examples/**/*.wasm` are
  tracked as runnable distributables.
- **Never commit or push without an explicit request.** Finishing a unit of
  work is not a request. Leave it in the working tree and report what changed.
- Commit messages: short imperative summary.

## Writing WAT

- Memory map in the file header comment.
- `;; @include <path>` splits a root source. Relative to the root's directory
  at every depth; never absolute, never `..`.
- Never hand-write a string address or length. Declare `;; @data
  <start>..<end>` once, leave named segments unplaced, read `$msg.ptr` /
  `$msg.len`. A segment stating a literal offset keeps it; the two may not
  overlap.
- Never hand-write the WASI 0.2 boundary. `;; @wasi <capabilities>` inside
  `(component` generates the imports, shared memory and canonical ABI
  lowering; `pages=` and `heap=` size it. Write against the `$wasi` and `$mem`
  core instances.
- Canonical ABI discriminants are `u8`: read with `i32.load8_u`, never
  `i32.load`.

## The generated boundary

- `filesystem`, `sockets`, `term` and `ui` derive their extent from the
  application's own `(import "fs" ...)` / `(import "net" ...)` / `(import
  "term" ...)` / `(import "ui" ...)` lines. An unknown name, or a capability
  with no import at all, fails the build.
- An import name is the WIT export key minus its bracketed kind:
  `descriptor.open-at`, `tcp-socket.subscribe`. A new interface is a table
  entry in `air/src/wit.rs`, never a transcribed type graph. The harness's own
  interfaces are in `air/wit/ai-direct-host/host.wit` — the one WIT file here
  that is not vendored, and the contract `component.rs` implements. Change
  both sides in that one file.
- Release handles with `<resource>.drop`: stream resources from `"wasi"`, a
  capability's own from its instance. Opt-in, and dropping something the
  boundary never declared fails the build.
- The heap is a bump pointer that frees nothing. A component that loops
  imports `"env" "heap-mark"` / `"heap-reset"` and resets per iteration, or
  dies with `realloc return: beyond end of memory`.
- `exit` takes a `result` — 0 or 1 only; for a status code ask for
  `exit-with-code` (`u8`).
- An interface `;; @wasi` does not name is still available: declare the import
  by hand. The directive is shorthand for the common boundary, never a gate.

## Manifests and capability

- One manifest per app, beside its modules, named `host.toml`. Paths resolve
  manifest-dir-first and travel with `air dist`.
- `target = "component"` (WASI 0.2) is the default and is inferred from the
  artifact. `browser` is the only other one; the native Preview 1 host has
  retired, so a prebuilt Core module is lifted with `wasm-tools component new`
  and consumed through `[[providers]]`, never linked directly.
- Nothing is reachable unless it asks. Directories: `root` / `[[dirs]]` in the
  manifest, `--dir` / `--dir-rw` from the shell, `write = true` for state.
  Sockets: `network = true` or `air run --net`. WASI has no global root, so an
  absolute path resolves nowhere by itself.
- A component consumes another through `[[providers]]`, wired at link time; no
  composition tool is involved. There is no host socket layer — a server is a
  component that owns its accept loop.
- `target` is the linking domain, `mode` the host loop. A GUI app is a
  component with `mode = "gui"` importing `ai-direct:host/ui`; `target = "gui"`
  does not exist.

`./build.sh` produces `dist/air`. Foreign `.wasm`? Start with `air inspect`.
