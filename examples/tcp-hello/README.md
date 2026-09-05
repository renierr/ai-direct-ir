# tcp-hello

A WASI 0.2 component that opens its own listening socket, serves HTTP requests
in a loop, and stops when one asks it to.

```bash
air run examples/tcp-hello/host.toml     # then, from another shell:
curl -i http://127.0.0.1:8125/
curl -i http://127.0.0.1:8125/           # again: the loop came back around
curl -i http://127.0.0.1:8125/quit       # answered, then the run ends
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
  declares 39 functions across seven interfaces. `tcp-hello.wat` imports nine
  of them plus two drops, and that is what the generated boundary declares — no
  UDP, no name lookup, no keep-alive timers, and no
  `wasi:clocks/monotonic-clock` that only the keep-alive setters would have
  needed. One `error-code` enum is declared by
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
- **A handle is released by the program that owns it.** Every resource the
  boundary declares offers `<resource>.drop`, and this is the example that
  needs it: an accepted connection is three handles, released before the next
  accept. The socket's drop is what closes the connection — comment it out and
  a client reading to end of stream waits forever.

## Layout

| Path | |
|---|---|
| `tcp-hello.wat` | the whole application: bind, listen, accept, answer |
| `host.toml` | `network = true` grants the sockets capability |

## Three handles per connection

`tcp-socket.accept` returns a tuple of three owned handles: the connected
socket, its `input-stream` and its `output-stream`. Nothing in the canonical
ABI releases them — a handle is the one thing the component itself has to give
back — so the loop ends with

```wat
(call $drop_out (local.get $out))
(call $drop_in (local.get $in))
(call $drop_socket (local.get $conn))
```

The stream resources come from `$wasi`, because stdio hands them out too; the
socket comes from `$net`. Both spell the drop the same way, `<resource>.drop`,
and both appear in the boundary only because the program imports them.

Dropping the socket is what closes the TCP connection, so the drops are load
bearing rather than tidy: without them the first client never sees end of
stream and the run leaks a handle per request.

## Giving the memory back too

Handles are not the only thing a loop owes. `blocking-read` allocates its
`list<u8>` through the boundary's `cabi_realloc`, which is a bump pointer and
frees nothing, so this example used to die on request 420:

```
Caused by: realloc return: beyond end of memory
```

The heap is one pointer, so the fix is to put it back:

```wat
(local.set $mark (call $heap_mark))   ;; top of the loop
...
(call $heap_reset (local.get $mark))  ;; bottom, after the last read
```

It now serves 5000 requests and exits 0. The reset comes *after* the `/quit`
check, because the request bytes it compares are themselves on the heap.

This is not a general allocator — it frees in reverse order or not at all,
which is exactly a request loop's shape and no use to a long-lived collection.
One `blocking-read` is also assumed to carry the whole request line, which is
true of every client here and not true in general.

## The fifteen-parameter bind

`tcp-socket.start-bind` takes an `ip-socket-address`, a variant of an IPv4 and
an IPv6 socket address. A variant passed by value is flattened into the
parameter list as one slot for the case plus the widest case's payload: IPv6
needs eleven, so every call passes twelve, and an IPv4 address leaves the last
six unread. The generated boundary spells the whole signature out; the header
comment in `tcp-hello.wat` records the memory map and the offsets that go with
it.
