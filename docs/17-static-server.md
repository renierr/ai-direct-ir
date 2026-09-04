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

## Why a host at all (and why it won't stay Python)

WASI preview1 — the only stable WASI the `wasmtime` CLI speaks — has
`sock_accept/recv/send` but **deliberately no `bind`/`listen`**, and the CLI
can't preopen a TCP listener either. So *some* host must own TCP and link the
two modules (see `docs/16-lib-reuse-linking.md`). `srv/serve.py` (~120 lines)
is that host and nothing more: **scaffolding, not architecture.**
The project goal is zero Python. Exit paths, easiest first:

1. **Tiny C host via libwasmtime** — same 5 imports + linker, ships as one
   small `srvhost` binary next to the `.wasm` files.
2. **WASI 0.2 `wasi:sockets`** — standard `bind/listen/accept` for
   components; then `wasmtime run server.wasm` needs no host code at all,
   and our custom `net.*` ABI disappears.

## Layout

- `lib/http.wat` → `lib/http.wasm` (1720 B): the "third-party" lib.
- `srv/server.wat` → `srv/server.wasm` (1420 B): the app.
- `srv/serve.py`: scaffolding host (dev machine only, never shipped).
- `srv/www/`: demo root (`index.html`, `style.css`, `hello.txt`, `data.json`).
- Assumes the single preopened dir lands on WASI fd 3 (true for one
  `--dir`/preopen; breaks if you add more — then pass the fd in).

## Run it

```bash
wat2wasm lib/http.wat -o lib/http.wasm
wat2wasm srv/server.wat -o srv/server.wasm
python3 srv/serve.py 8123   # needs wasmtime pip package (dev only)
```

## Verified behavior (curl matrix, 2026-09-04)

| Request | Result |
|---|---|
| `GET /` | 200 `text/html; charset=utf-8`, 570 B, correct headers |
| `GET /style.css`, `/hello.txt`, `/data.json` | 200, correct MIME each |
| `GET /missing` | 404 |
| `GET /../src/pi.wat`, `/a/../../etc/passwd` (`--path-as-is`) | 403 (curl normalizes `/../` by default — retest raw!) |
| `POST /` | 405 |
| `GET /hello.txt?v=2` | 200 (query stripped) |

Single connection at a time, `Connection: close`, requests must fit 8 KiB
(else 400). Enough for a proof; keep-alive/threading are later.
