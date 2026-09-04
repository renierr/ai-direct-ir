# Static file server in hand-written IR (proof)

`GET /` from `srv/www/` answered by ~140 lines of WAT across two modules —
HTTP parsing, routing, MIME sniffing, header building and file I/O all
execute inside WebAssembly. Verified with curl (matrix below).

```
                         +------------------ srv/server.wasm
                         |  accept loop, request routing, ".." guard,
   TCP bytes             |  path_open/fd_read/fd_filestat_get (WASI files)
   <-------------------> |  lib.* for everything HTTP-shaped
  net.* syscalls         +--------------------------------------
  (host, swappable)      +------------------ lib/http.wasm
                         |  parse_request, send_status/header/clen/crlf,
  files                  |  mime_for, send_all  (no WASI, no app imports)
  <--------------------> |
   WASI fd 3 = srv/www/  +--------------------------------------
```

## Host: `host-rs/` (Rust — no Python at runtime)

WASI preview1 — the only stable WASI the `wasmtime` CLI speaks — has
`sock_accept/recv/send` but **deliberately no `bind`/`listen`**, and the CLI
can't preopen a TCP listener either. So *some* host must own TCP and link
the modules (see `docs/16-lib-reuse-linking.md`). That host is `host-rs/`
(~200 lines of Rust on wasmtime 48): **scaffolding, not architecture.**
`srv/serve.py` did the same job in Python and is retired (kept as reference).

The host does exactly three things — own the ONE shared memory (`env`),
implement five `net.*` syscalls over `std::net` sockets, link
`lib/http.wasm` → app, plus bridge the Rust lib (below). All HTTP parsing,
routing and file serving run 100% inside the `.wasm` modules.
Further exit path if even this host feels heavy: WASI 0.2 `wasi:sockets`
(standard bind/listen — then `wasmtime run` needs no host code at all,
and our custom `net.*` ABI disappears).

## Layout

- `lib/http.wat` → `lib/http.wasm` (1720 B): the hand-written lib.
- `lib-sha256/` → `lib/sha256.wasm` (63 KB): finished crates.io `sha2`,
  exposed via `sha256_alloc`/`sha256_hex`, bridged by the host
  (see `docs/18-cargo-libs.md`).
- `srv/server.wat` → `srv/server.wasm`: the app, incl. `POST /sha256`.
- `host-rs/`: the harness (dev + ship; one native binary + three `.wasm`).
- `srv/www/`: demo root (`index.html`, `style.css`, `hello.txt`, `data.json`).
- Assumes the single preopened dir lands on WASI fd 3 (true for one
  preopen; breaks if you add more — then pass the fd in).

## Run it (no Python — via the generic harness, `docs/19-harness.md`)

```bash
wat2wasm lib/http.wat -o lib/http.wasm
wat2wasm srv/server.wat -o srv/server.wasm
cargo build --release --target wasm32-wasip1   # from lib-sha256/
cp lib-sha256/target/wasm32-wasip1/release/sha256.wasm lib/
cargo build --release                          # from host-rs/ (once)
./host-rs/target/release/host-rs srv/manifest.toml
```

## Verified behavior (curl matrix, 2026-09-04, Rust host)

| Request | Result |
|---|---|
| `GET /` | 200 `text/html; charset=utf-8`, 570 B, correct headers |
| `GET /style.css`, `/hello.txt`, `/data.json` | 200, correct MIME each |
| `GET /missing` | 404 |
| `GET /../src/pi.wat`, `/a/../../etc/passwd` (`--path-as-is`) | 403 (curl normalizes `/../` by default — retest raw!) |
| `POST /` | 405 |
| `GET /hello.txt?v=2` | 200 (query stripped) |
| `POST /sha256` body `abc` | `ba7816bf…15ad`, identical to `sha256sum` |
| `POST /sha256` 5 KB random blob | identical to `sha256sum` |
| `GET /sha256` | 404 (not a file; hash is POST-only) |

Single connection at a time, `Connection: close`, requests must fit 8 KiB
(else 400), hash bodies ≤ 7 KiB. Enough for a proof; keep-alive/threading
are later.
