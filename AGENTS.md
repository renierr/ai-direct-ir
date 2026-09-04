# AGENTS.md — rules for coding agents in this repo

## Goal

`host-rs` is the product: a generic harness that links + hosts configured
WASM apps and libs. Everything else (examples, libs, docs) exists to prove
and use it. AI writes the IR (`.wat`); the harness runs it.

## Hard rules

- **Never install, upgrade, or remove software without explicit user consent.**
  Missing tool? Stop and ask. (`rustup target add …`, `pip install …`, etc.)
- **Verify by execution.** Every claim ends in a run: `wat2wasm` must
  assemble, `host-rs check` must pass, `curl`/CLI output must match.
  Raw bytes over pretty output (`curl -i`; curl hides NULs).
- **Keep the harness generic.** New app needs go in the manifest, never in
  `host-rs` code. New ABI shapes (syscalls, bridge arities) extend the host
  once so all apps benefit.
- **Generated = ignored.** Track sources (`.wat`, `.rs`, `.toml`, `.md`);
  never commit `target/`, `*.o`, or lib `*.wasm`. Exception:
  `examples/**/*.wasm` are tracked as runnable distributables.
- **Log experiments** in `docs/06-lablog.md` (newest first):
  Goal / Command / Output / Learning. Findings that stick go in `docs/`.

## Layout

- `host-rs/src/` — `main.rs` (CLI) + `manifest`/`host`/`net`/`link`/`cmds`
- `examples/<name>/` — `<name>.wat` + tracked `<name>.wasm` + `<name>.toml`
- `examples/server/` — `server.wat`, `manifest.toml`, `www/` demo root
- `libs/http/` — hand-written WAT lib; `libs/sha256/` — Rust crate (`sha2`)
- `native/` — wasm2c experiments; `tools/` — retired Python host; `docs/`

## Build / run (from repo root unless noted)

```bash
wat2wasm libs/http/http.wat -o libs/http/http.wasm
wat2wasm examples/server/server.wat -o examples/server/server.wasm
cargo build --release --target wasm32-wasip1   # from libs/sha256/
cp libs/sha256/target/wasm32-wasip1/release/sha256.wasm libs/sha256/
cargo build --release                          # from host-rs/ (harness, once)
./host-rs/target/release/host-rs check examples/server/manifest.toml
./host-rs/target/release/host-rs examples/server/manifest.toml  # :8124
echo 100 | ./host-rs/target/release/host-rs examples/pi/pi.toml
```

Manifest paths resolve manifest-dir-first (relocatable); full reference in
`docs/19-harness.md`. Foreign `.wasm`? Start with
`host-rs inspect <file>` (`docs/18-cargo-libs.md`).

## Conventions

- WAT: memory map in the file header comment; lib address maps are ABI.
- Manifest per app, next to its modules; `host-rs init` scaffolds it.
- Commit messages: short imperative summary. Push when asked or when a
  unit of work completes.
