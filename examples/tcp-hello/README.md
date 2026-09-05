# tcp-hello

A WASI 0.2 component that opens its own listening socket, serves one HTTP
request, and exits.

```bash
air run examples/tcp-hello/host.toml     # then, from another shell:
curl -i http://127.0.0.1:8125/
```

```
HTTP/1.1 200 OK
Content-Type: text/plain; charset=utf-8
Content-Length: 12
Connection: close

hello, air!
```

## What it proves

- **`wasi:sockets` from WIT, at the size the program asks for.** The WIT
  declares 39 functions across seven interfaces. `tcp-hello.wat` imports nine,
  and nine is what the generated boundary declares — no UDP, no name lookup,
  no keep-alive timers, and no `wasi:clocks/monotonic-clock` that only the
  keep-alive setters would have needed. One `error-code` enum is declared by
  `wasi:sockets/network` and aliased into the interfaces that share it.
- **A method is named by its resource.** `tcp-socket.subscribe`, not
  `subscribe`: five different resources in `wasi:sockets` have one. The
  import name is the WIT export key with its `[method]` prefix dropped.
- **The network is a grant, like a directory.** `wasi:sockets` is linked for
  every component, and every call answers `access-denied` until the host says
  otherwise. Delete `network = true` from `host.toml` and the run stops at
  `create-tcp-socket` with error-code 1. `air run --net` is the shell-side
  equivalent, for a manifest that does not ask.
- **The guest owns the accept loop.** `air`'s `mode = "server"` and its
  `net.*` host syscalls exist because Core WASM had no sockets; nothing in
  this example goes through them. WASI 0.2 has no blocking accept, so the
  program subscribes to the socket and blocks on the `pollable`.

## Layout

| Path | |
|---|---|
| `tcp-hello.wat` | the whole application: bind, listen, accept, answer |
| `host.toml` | `network = true` grants the sockets capability |

## Why one connection

Every accepted connection is three resource handles — the socket and its two
streams — and dropping them needs a `(canon resource.drop ...)` per type. A
program that serves one request and exits lets the store drop them instead,
which keeps the example about sockets. A long-running server needs the drops,
and that is the next thing this example would grow.

## The fifteen-parameter bind

`tcp-socket.start-bind` takes an `ip-socket-address`, a variant of an IPv4 and
an IPv6 socket address. A variant passed by value is flattened into the
parameter list as one slot for the case plus the widest case's payload: IPv6
needs eleven, so every call passes twelve, and an IPv4 address leaves the last
six unread. The generated boundary spells the whole signature out; the header
comment in `tcp-hello.wat` records the memory map and the offsets that go with
it.
