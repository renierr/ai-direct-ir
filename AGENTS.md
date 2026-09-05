# AGENTS.md — rules for coding agents in this repo

## Goal

`air` is the product: a generic harness that links + hosts configured
WASM apps and libs. Everything else (examples, libs, docs) exists to prove
and use it. AI writes the IR (`.wat`); the harness runs it.

Read `docs/PROJECT.md` before starting work. It is the living project
documentation shared with dependent sibling projects and the source of truth
for implemented behavior, current limitations, the three-repository split, and
ordered next milestones.

## Repository Role

This is the generic platform repository. Work here when an example application
reveals a reusable need in composition, validation, lifecycle, packaging,
permissions, or a truly host-owned effect. Put mature library adapters in the
sibling `ai-direct-ir-providers` catalog and application behavior in an example
repository such as `ai-direct-ir-example-mail`. The example may break while the
generic design changes.

## Hard rules

- **Never install, upgrade, or remove software without explicit user consent.**
  Missing tool? Stop and ask. (`rustup target add …`, `pip install …`, etc.)
- **Verify by execution.** Every claim ends in a run: `cargo test` must pass,
  `air` must assemble, `air check` must pass, `curl`/CLI output must
  match. Raw bytes over pretty output (`curl -i`; curl hides NULs).
  A behavior worth claiming is worth a test in `air/tests/cli.rs`.
- **Keep the harness generic.** New app needs go in the manifest, never in
  `air` code. New ABI shapes (syscalls, bridge arities) extend the host
  once so all apps benefit.
- **Builder phase: redesign freely.** Update `docs/PROJECT.md` with
  every host capability change. Current Core ABI and GUI/browser imports are
  experimental; replace them directly when WIT/Component Model composition is
  better. Do not add compatibility layers without a real consumer.
- **Use example apps as integration drivers.** When an application reveals a
  generic missing capability, improve the harness or provider composition with
  it. Keep the application allowed to break while the experimental interface is
  redesigned; never add an application-specific shortcut to unblock it.
- **Generated = ignored.** Track sources (`.wat`, `.rs`, `.toml`, `.md`);
  never commit `target/`, `*.o`, or lib `*.wasm`. Exception:
  `examples/**/*.wasm` are tracked as runnable distributables.

## Layout

- `air/src/` — `main.rs` (CLI) + `manifest`/`host`/`net`/`link`/`cmds`
- `air/tests/cli.rs` — end-to-end tests that drive the real binary
- `examples/<name>/` — `<name>.wat` + tracked `<name>.wasm` + `host.toml`;
  every manifest declares `source`, so the artifact is rebuilt from the WAT
- `examples/server/` — `server.wat`, `manifest.toml`, `www/` demo root
- `libs/http/` — hand-written WAT lib; `libs/sha256/` (`sha2`) and
  `libs/text-width/` (`unicode-width`) — Rust crates
- `native/` and `tools/` — legacy experiments; `docs/PROJECT.md` — living state

## Build / run (from repo root unless noted)

```bash
cargo build --release --target wasm32-wasip1   # from libs/sha256/
cp libs/sha256/target/wasm32-wasip1/release/sha256.wasm libs/sha256/
cargo test --manifest-path air/Cargo.toml
./build.sh
./dist/air check examples/server/manifest.toml
./dist/air run examples/hello/host.toml
./dist/air examples/server/manifest.toml  # :8124
echo 100 | ./dist/air run examples/pi/host.toml
```

Manifest paths resolve manifest-dir-first (relocatable). Foreign `.wasm`?
Start with `air inspect <file>`.

## Conventions

- WAT: memory map in the file header comment; lib address maps are ABI.
- `;; @include <path>` splits a root WAT into fragments. Paths are relative to
  the root source's directory at every depth, never absolute and never `..`.
- Never hand-write a string length. Name the segment — `(data $msg (i32.const
  0x1000) "...")` — and read `$msg.ptr` / `$msg.len`, which `air` derives.
  Named segments need a literal offset and may not overlap.
- Manifest per app, next to its modules; `air init` scaffolds it. Name it
  `host.toml` so the project directory is the argument.
- Prefer `target = "component"` (WASI 0.2) for new work; it is the default and
  is inferred from the artifact, so a manifest rarely states it. Reach for
  `native` (Preview 1) only for Core providers or a pointer-passing host ABI.
- Never hand-write the WASI 0.2 boundary. `;; @wasi stdin stdout stderr exit`
  inside `(component` generates the imports, the shared memory and the
  canonical ABI lowering; `pages=` and `heap=` size the memory. Write the
  program against the `$wasi` and `$mem` core instances it defines.
- A component consumes another component through `[[providers]]`, wired at link
  time. No composition tool is involved.
- Commit messages: short imperative summary.
- **Never commit or push without an explicit request.** Finishing a unit of
  work is not a request. Leave changes in the working tree, report what
  changed, and let the user decide when it lands.
