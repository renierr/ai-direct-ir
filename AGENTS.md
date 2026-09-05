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

- `air/src/` — `main.rs` (CLI) + `manifest`/`host`/`net`/`link`,
  `cmds/` (one file per subcommand) and `asm/` (the WAT assembler:
  `source` expands, `scan` reads, `data` places segments, `diag` maps a
  failure back to the authored line). `boundary`/`wit` generate the
  WASI boundary `asm/source` expands.
- `air/tests/cli.rs` — end-to-end tests that drive the real binary
- `examples/<name>/` — `<name>.wat` + tracked `<name>.wasm` + `host.toml`;
  every manifest declares `source`, so the artifact is rebuilt from the WAT
- `examples/server/` — `server.wat`, `manifest.toml`, `www/` demo root
- `examples/tcp-hello/` — a component that binds its own socket through
  `wasi:sockets` and serves an accept loop; `examples/sha256sum/` — the same
  for `wasi:filesystem`
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
- Never hand-write a string length or a string address. Name the segment and
  read `$msg.ptr` / `$msg.len`, which `air` derives. Declare `;; @data
  <start>..<end>` once and leave named segments unplaced — `(data $msg "...")`
  — and `air` packs them in source order. A segment that states a literal
  offset keeps it; placed and unplaced segments may not overlap.
- Manifest per app, next to its modules; `air init` scaffolds it. Name it
  `host.toml` so the project directory is the argument.
- Prefer `target = "component"` (WASI 0.2) for new work; it is the default and
  is inferred from the artifact, so a manifest rarely states it. Reach for
  `native` (Preview 1) only for Core providers or a pointer-passing host ABI.
- Never hand-write the WASI 0.2 boundary. `;; @wasi stdin stdout stderr exit`
  inside `(component` generates the imports, the shared memory and the
  canonical ABI lowering; `pages=` and `heap=` size the memory. Write the
  program against the `$wasi` and `$mem` core instances it defines.
- `exit` takes a `result`: 0 or 1, nothing else, so it says only whether the
  run failed. For a POSIX-style status ask for `exit-with-code` (`u8`).
  `args` imports `wasi:cli/environment`; `air run <manifest> <args...>`
  forwards everything after the manifest, with argv[0] the app name.
- A component reads only granted directories. Manifest paths (`root`,
  `[[dirs]]`) are project-relative and travel with `air dist`; `--dir` /
  `--dir-rw` are relative to the shell. Host options come before the manifest.
  WASI has no global root, so an absolute path resolves nowhere by itself.
- Nothing is writable unless it asks. A stateful app declares
  `[[dirs]] path = "data"` with `write = true`, which `air` creates on first
  run; that is where a database or cache belongs.
- `;; @wasi filesystem` and `;; @wasi sockets` read the application's
  `(import "fs" "...")` / `(import "net" "...")` lines and generate exactly
  those functions from the vendored WASI WIT. Import what the program calls; a
  name the WIT does not have, or a capability with no import at all, fails the
  build.
- An import name is the WIT export key with its bracketed kind dropped:
  `descriptor.open-at`, `tcp-socket.subscribe`, `get-directories`. Methods are
  qualified by their resource because five `wasi:sockets` resources have a
  `subscribe`. Adding an interface is a table entry in `air/src/wit.rs`, never
  a transcribed type graph.
- Nothing is reachable unless it asks. `wasi:sockets` is linked for every
  component and answers `access-denied` until `network = true` in the manifest
  or `air run --net` grants it — the same rule as `[[dirs]]` and `--dir`.
- A resource the boundary declares can be released: import `<resource>.drop`.
  The stream resources come from `"wasi"` (`"input-stream.drop"`), a
  capability's own from its instance (`"tcp-socket.drop"`). It is opt-in like
  every other name, and dropping something the boundary never declared fails
  the build. `examples/tcp-hello/` shows why it matters: dropping the accepted
  socket is what closes the connection.
- A WASI interface `;; @wasi` does not name is still available: declare the
  import by hand. `air` links the whole WASI 0.2 set, so the directive is a
  shorthand for the common boundary, never a gate. See `examples/sha256sum/`.
- Canonical ABI discriminants are `u8`. Read them with `i32.load8_u`; an
  `i32.load` takes three bytes of undefined padding along with the tag.
- A component consumes another component through `[[providers]]`, wired at link
  time. No composition tool is involved.
- Commit messages: short imperative summary.
- **Never commit or push without an explicit request.** Finishing a unit of
  work is not a request. Leave changes in the working tree, report what
  changed, and let the user decide when it lands.
