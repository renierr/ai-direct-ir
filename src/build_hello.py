#!/usr/bin/env python3
"""Build hello.wasm directly (no wat2wasm needed) — AI-direct IR demo.

Emits the same module as src/hello.wat:
  import wasi_snapshot_preview1.fd_write, memory(1), export _start
  writes "hello from AI-direct IR\\n" to stdout.
"""
import pathlib


def uleb(n: int) -> bytes:
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def section(sid: int, payload: bytes) -> bytes:
    return bytes([sid]) + uleb(len(payload)) + payload


def str_b(s: str) -> bytes:
    b = s.encode()
    return uleb(len(b)) + b


msg = b"hello from AI-direct IR\n"  # 24 bytes
assert len(msg) == 24, len(msg)

# --- type section: 2 types ---
# type0: func(i32,i32,i32,i32)->i32   (fd_write)
# type1: func()->()                   (_start)
type_sec = section(1, uleb(2)
    + b"\x60" + uleb(4) + b"\x7f\x7f\x7f\x7f" + uleb(1) + b"\x7f"
    + b"\x60" + uleb(0) + uleb(0))

# --- import section: 1 import ---
imp = str_b("wasi_snapshot_preview1") + str_b("fd_write") + b"\x00" + uleb(0)
import_sec = section(2, uleb(1) + imp)

# --- function section: 1 defined func (index 1), type 1 ---
func_sec = section(3, uleb(1) + uleb(1))

# --- memory section: 1 memory, min 1 page ---
mem_sec = section(5, uleb(1) + b"\x00" + uleb(1))

# --- export section: memory 0 as "memory", func 1 as "_start" ---
exp = (uleb(2)
    + str_b("memory") + b"\x02" + uleb(0)
    + str_b("_start") + b"\x00" + uleb(1))
export_sec = section(7, exp)

# --- code section: body of _start ---
body = (
    b"\x00"  # 0 locals
    b"\x41\x00" + b"\x41\x08" + b"\x36\x02\x00"      # store buf ptr 8 at 0
    b"\x41\x04" + b"\x41\x18" + b"\x36\x02\x00"      # store len 24 at 4
    b"\x41\x01" + b"\x41\x00" + b"\x41\x01" + b"\x41\x20"  # args 1,0,1,32
    + b"\x10\x00"  # call func 0 (fd_write)
    + b"\x1a"      # drop errno
    + b"\x0b"      # end
)
code_sec = section(10, uleb(1) + uleb(len(body)) + body)

# --- data section: 1 active segment at offset 8 ---
seg = (b"\x00"  # flags: active, no memory index
    + b"\x41\x08\x0b"  # i32.const 8; end
    + uleb(len(msg)) + msg)
data_sec = section(11, uleb(1) + seg)

wasm = b"\x00asm" + b"\x01\x00\x00\x00" + type_sec + import_sec + func_sec + mem_sec + export_sec + code_sec + data_sec

out = pathlib.Path(__file__).with_name("hello.wasm")
out.write_bytes(wasm)
print(f"wrote {out} ({len(wasm)} bytes)")
