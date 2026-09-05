#!/usr/bin/env python3
"""Scaffolding host for the AI-direct-IR static file server.

SUPERSEDED by air/ (same job in Rust, no Python needed at runtime).
Kept as the readable reference implementation of the harness pattern.

SCAFFOLDING, not architecture. This file does exactly two swappable jobs:
  1. Loader: instantiates lib/http.wasm first, plugs its exports into
     srv/server.wasm's lib.* imports, both sharing ONE host-owned memory
     (imported as "env"). Core WASM has no static linker; any host —
     Python, Rust, C, Go — can do this job.
  2. Syscalls: implements net.listen/accept/recv/send/close over real OS
     sockets. WASI preview1 (what `wasmtime run` gives you) deliberately has
     no bind/listen, so a host must provide TCP. Same role as an OS kernel.

All HTTP parsing, routing, MIME sniffing, header building and file serving
run 100% inside the two .wasm modules. Exit paths to drop Python entirely:
a tiny C host via libwasmtime, or WASI 0.2 wasi:sockets (standard bind/listen
— then `wasmtime run` needs no host code at all). See docs/17-static-server.md.

Usage:  python3 tools/serve.py [port]      (www/ root, serves examples/server/www/)
Requires the wasmtime Python package (dev-machine only, NOT shipped).
"""
import socket
import sys

from wasmtime import (
    Config,
    Engine,
    Func,
    FuncType,
    Limits,
    Linker,
    Memory,
    MemoryType,
    Module,
    Store,
    ValType,
    WasiConfig,
)

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8123
I32 = ValType.i32()

store = Store(Engine(Config()))
wasi = WasiConfig()
wasi.preopen_dir("examples/server/www", "www")  # single preopen -> WASI fd 3, see server.wat
store.set_wasi(wasi)

linker = Linker(store.engine)
linker.define_wasi()

# One shared memory, owned by the host, imported by both modules as "env".
mem = Memory(store, MemoryType(Limits(2, 2), False))
linker.define(store, "env", "memory", mem)

socks: dict = {}
next_handle = [100]


def _new_sock(s: socket.socket) -> int:
    h = next_handle[0]
    next_handle[0] += 1
    socks[h] = s
    return h


def w_listen(caller, port: int) -> int:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind(("127.0.0.1", port))
        s.listen(16)
    except OSError:
        s.close()
        return -1
    return _new_sock(s)


def w_accept(caller, h: int) -> int:
    try:
        conn, _ = socks[h].accept()
    except (OSError, KeyError):
        return -1
    return _new_sock(conn)


def w_recv(caller, h: int, ptr: int, length: int) -> int:
    try:
        data = socks[h].recv(min(length, 65536))
    except (OSError, KeyError):
        return -1
    if not data:
        return 0
    mem.write(caller, data, ptr)
    return len(data)


def w_send(caller, h: int, ptr: int, length: int) -> int:
    try:
        return socks[h].send(mem.read(caller, ptr, ptr + length))
    except (OSError, KeyError):
        return -1


def w_close(caller, h: int) -> int:
    s = socks.pop(h, None)
    if s is not None:
        s.close()
    return 0


linker.define(
    store,
    "net",
    "listen",
    Func(store, FuncType([I32], [I32]), w_listen, access_caller=True),
)
linker.define(
    store,
    "net",
    "accept",
    Func(store, FuncType([I32], [I32]), w_accept, access_caller=True),
)
linker.define(
    store,
    "net",
    "recv",
    Func(store, FuncType([I32, I32, I32], [I32]), w_recv, access_caller=True),
)
linker.define(
    store,
    "net",
    "send",
    Func(store, FuncType([I32, I32, I32], [I32]), w_send, access_caller=True),
)
linker.define(
    store,
    "net",
    "close",
    Func(store, FuncType([I32], [I32]), w_close, access_caller=True),
)

# Lib first (needs only env.memory + net.send), then app (needs lib.* too).
lib = linker.instantiate(store, Module.from_file(store.engine, "libs/http/http.wasm"))
for name, ext in lib.exports(store).items():
    linker.define(store, "lib", name, ext)

app = linker.instantiate(store, Module.from_file(store.engine, "examples/server/server.wasm"))
run = app.exports(store)["run"]

print(f"serving srv/www/ on 127.0.0.1:{PORT} (Ctrl-C to stop)", flush=True)
run(store, PORT)
