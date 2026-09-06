# server

A static file server as a WASI 0.2 component. It owns its accept loop, reads
one granted directory, and gets its digest from a vendored provider component.

```bash
air run examples/server/host.toml          # :8124
curl -i http://127.0.0.1:8124/                 # www/index.html
curl -sS --data-binary abc http://127.0.0.1:8124/sha256
curl -sS http://127.0.0.1:8124/quit            # stops the run
```

## What it proves

- **A server needs no host socket layer.** This example used to depend on five
  `net.*` host syscalls and `mode = "server"`. Both are gone: `;; @wasi
  sockets` generates the boundary and the guest runs `accept` itself. Nothing
  in `air` knows this application is a server.
- **One catalog package, two applications.** The digest comes from the same
  vendored `ai-direct:sha256` provider component `examples/sha256sum/`
  consumes, through one `[[providers]]` line. The Core `[[bridges]]` block it
  replaced needed four byte offsets and a `max_in`.
- **Handles are released per request.** A connection is three handles and a
  served file is two more. Dropping the accepted socket is what closes the
  connection; dropping the descriptor and stream is what keeps a long run from
  accumulating one of each per request.
- **The heap is reclaimed per request.** Every host-produced value — the
  request bytes, each file chunk, the digest string — is allocated from a bump
  heap that frees nothing. A mark at the top of the loop and a reset at the
  bottom bound the whole run to one connection's allocation.
- **Two grants, no ambient authority.** `network = true` lets it bind;
  `root = "www"` is the only directory it can read. Without the first, the run
  stops at `create-tcp-socket`. Without the second it serves nothing, and
  `/../AGENTS.md` answers 403 either way.

## Layout

| Path | |
|---|---|
| `server.wat` | the accept loop, routing, and file serving |
| `src/http.wat` | response building and request parsing, `;; @include`d |
| `host.toml` | the two grants and the provider |
| `www/` | the demo document root |

## Routes

| | |
|---|---|
| `GET /<path>` | a file under `www/`, with a Content-Type from its extension |
| `GET /hello` | a fixed 12-byte body from memory, for benchmarking the transport |
| `GET /quit` | answers, then ends the run |
| `POST /sha256` | the request body, hex-digested by the provider |

## What still limits it

One connection at a time, and one `blocking-read` is assumed to carry the
whole request. Serving concurrently would mean `wasi:io/poll`'s
`poll(list<pollable>)` over many connections — expressible on this path, which
is part of why it replaced `net.*`, but not done here.

`src/http.wat` is a fragment rather than a provider because a provider cannot
take a handle, and half of it writes to an `output-stream`. The pure half —
`$parse_request` and `$mime_for` — is the natural next catalog package.
