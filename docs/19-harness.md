# The generic harness (`host-rs`)

One native binary runs every app in this repo. A new project = a TOML
manifest + `.wasm` files. The harness is never rebuilt for a new app —
exactly the "config, not code" property the user asked for.

## Run

Run from the repo root (manifest paths resolve manifest-dir first,
process dir second):

```bash
cargo build --release                        # from host-rs/ — once, 25 MB binary
host-rs examples/server/manifest.toml        # server mode
host-rs examples/pi/pi.toml                  # command mode
echo 100 | host-rs examples/pi/pi.toml
host-rs examples/hello/hello.toml
```

For a scaffolded project, `host.toml` declares `[app].source` and `[app].path`.
From its directory, build, validate, and run with:

```bash
host-rs build
host-rs check
host-rs run
```

Ship shape per app: `host-rs` + the `.wasm` files + data dir. The binary is
per-OS (25 MB release); the `.wasm` files are portable.

## Manifest reference

```toml
mode = "server"      # or "command"
port = 8124          # server mode (default 8123)
root = "www"            # optional preopen dir (WASI fd 3 when alone)
guest = "www"        # optional guest name (default: root's file name)
memory_pages = 2     # optional floor; import minima always win
workers = 8          # server only: host-owned accept loop + N instances.
                     # absent/1 = legacy: the app's `run` owns listen+accept.
                     # worker mode calls `run` as handle(cfd) per connection.

[[libs]]             # shared-memory libs: every export auto-wired
path = "libs/http/http.wasm"
as = "lib"           # namespace the app imports from

[[bridges]]          # own-memory libs: host copies buffers across
path = "libs/sha256/sha256.wasm"
as = "bridge"
alloc = "sha256_alloc"

[[bridges.calls]]    # v1 shape: (in_ptr, in_len, out_ptr) -> rc
as = "sha256"
func = "sha256_hex"
in_ptr = 0
in_len = 1
out_ptr = 2
out_len = 64
max_in = 7000        # optional input cap (default 1 MiB)

[app]
source = "server.wat" # optional: WAT source for `host-rs build`
path = "server.wasm"
run = "run"          # server: run(port); command: run() e.g. _start
```

## What the harness does, in order

1. Parses the manifest; builds the WASI ctx (stdio inherited, optional
   single preopen read-only).
2. Creates the shared `env.memory` **iff** some module imports it, sized to
   `max(memory_pages, every importer's declared minimum)` — a 1-page
   request against a `(memory 2)` import fails loudly instead of
   mysteriously.
3. Defines `net.*` (TCP listen/accept/recv/send/close over `std::net`).
4. Instantiates `[[libs]]` first, auto-wiring **all** their exports under
   `as` (the export list is the contract — no hardcoded names).
5. Instantiates `[[bridges]]`, wrapping each call in a host copier
   (app-mem → lib `alloc` → call → copy back).
6. Instantiates the app and calls `run`: `run(port)` for servers,
   `run()` for commands (`proc_exit` code becomes the process exit code).

## Current manifests

| Manifest | Mode | Modules |
|---|---|---|
| `examples/server/manifest.toml` | server :8124 | app + http lib + sha256 bridge |
| `examples/server/mt.toml` | server :8124, 8 workers | same modules, entry `handle` |
| `examples/pi/pi.toml` | command | app only (`_start`, stdio) |
| `examples/hello/hello.toml` | command | app only |

## v1 limits (extend once, all apps benefit)

- Bridge calls are fixed-arity `(in_ptr, in_len, out_ptr) -> rc`.
- Syscalls are TCP client/server + WASI files/stdio. No UDP/timers yet.
- `term.*` adds an optional raw-terminal capability (key events, alternate
  screen, cursor, size); see `docs/21-terminal.md`. It rejects pipes so apps
  must keep an ordinary stdio fallback.
- Single preopen dir. Single connection at a time is the *app's* choice,
  not the harness's — or set `workers` and let the host parallelize.

## Workers mode (host-owned accept loop)

`workers = N` keeps the manifest + modules identical and changes who loops:
the main thread accepts, N worker threads each own a fully linked instance
(own `Store`, own `env.memory`, own WASI ctx) and run the manifest's `run`
entry as `handle(cfd)` per connection. Blocking sockets + OS threads, std
only — no async runtime, no new deps. The same `server.wasm` serves both
modes (`run` owns the loop, `handle` serves one connection; the host closes
the socket after return, so a leaky app can't exhaust the table). A
trapping worker costs its connection, not the server — in legacy mode a
trap kills the whole loop.

Measured (loopback, same bench script): sequential single 4320 rps/0.22 ms
vs workers 3505 rps/0.28 ms (+~60 µs channel dispatch per request), but two
simultaneous 8-thread clients aggregate ~9.8k rps single (server saturated)
vs ~10.6k workers (headroom left). Raw per-request service is ~0.1 ms —
sequential latency includes ~0.1 ms of Python client overhead each side.
