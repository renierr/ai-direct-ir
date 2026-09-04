# Finished libs from any language (proven: crates.io `sha2`)

The harness model is language-agnostic: **any toolchain that emits a core
WASM module plugs in.** The host only loads, links (imports↔exports) and
provides syscalls. What we proved: a finished crates.io crate (`sha2`
0.10, pulled via cargo like any dependency) hashing inside our server —
`POST /sha256` returns the identical digest as `sha256sum`, including a
5 KB random blob.

## What a lib module must satisfy

1. **Core module output.** `wasm32-wasip1` (Rust), wasi-sdk/clang (C/C++),
   TinyGo wasi, AssemblyScript, Zig — all fine. NOT components, NOT
   JS-glued output (mainline Go `GOOS=js`, wasm-bindgen without care),
   NOT npm/PyPI packages (different VMs entirely).
2. **Explicit imports/exports.** Needs in, API out. Our contract:
   `sha256_alloc(u32)->ptr`, `sha256_hex(ptr,len,out64)->i32`.
3. **A memory strategy** (the only real question — see below).
4. **Compatible WASI use.** Our linker already provides preview1; whatever
   the lib imports must be defined by the harness.

## The two memory strategies

| | Shared memory (lib/http.wasm) | Copied bridge (lib/sha256.wasm) |
|---|---|---|
| Lib memory | None — uses host's | Own (Rust `std` assumes ownership) |
| Pointers | Valid everywhere | Valid only inside the lib |
| Host work | Zero per call | `memcpy` in, call, `memcpy` out |
| Needs | Lib accepts imported memory (WAT, C with `--import-memory`, `no_std` Rust) | Nothing — works with ANY lib, incl. `std` Rust |

Rule: share when the toolchain allows it, bridge-copy otherwise. Copying
is a few microseconds — irrelevant next to TCP.

## Recipe: adding a cargo lib (what we did)

1. `cargo new --lib lib-foo`; add `[lib] crate-type = ["cdylib"]`,
   `name = "foo"`; depend on the finished crate normally.
2. Expose `#[no_mangle] pub extern "C"` fns + an allocator (ours leaks
   `Vec`s on purpose — the host owns lifetimes).
3. `cargo build --release --target wasm32-wasip1` (needs
   `rustup target add wasm32-wasip1`, one-time), copy to `lib/foo.wasm`.
4. Host: instantiate (wasi imports already defined), stash its `Memory` +
   `TypedFunc` handles, `func_wrap` a `bridge.*` function the app imports.
5. App (WAT): `(import "bridge" "foo" ...)` + an endpoint. Done.

`no_std` + imported memory would graduate a lib from row 2 to row 1 —
worth doing for hot paths, unnecessary for correctness.

## Language notes

- **Rust**: the easy case (this doc). `std` works on `wasm32-wasip1`.
- **C/C++**: wasi-sdk/clang, `--import-memory` for zero-copy sharing.
- **Go**: mainline Go targets JS only — wrong shape. TinyGo `-target=wasi`
  emits linkable core modules.
- **JS/npm, Python packages**: no. (A whole language runtime compiled to
  wasm, e.g. QuickJS-in-wasm, could be linked as one fat lib — possible,
  heavyweight, later.)
