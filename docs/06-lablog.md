# Lab Log — Experiments

Append newest first. Template per entry: Goal / Command / Output / Learning.

## 2026-09-04 — Workers mode: host accept loop + N instances; ceiling mapped
- `host-rs`: `workers = N` in manifest (default 1 = legacy in-app loop). Main thread accepts, N workers with own Store/instance run `run` as `handle(cfd)`; host closes after return; trap kills a connection, not the server. `server.wat` exports `handle` alongside `run` — same `.wasm` serves `manifest.toml` and `mt.toml`.
- Numbers: seq single 4320/0.22ms vs workers-8 3505/0.28ms (+60µs dispatch); dual 8-thread clients aggregate 9.8k single (saturated) vs 10.6k workers (headroom). True service ~0.1ms; sequential latency carries ~0.1ms of Python client overhead.
- Debugging scars: `pgrep -f`/`pkill` patterns that appear in the wrapper's own cmdline suicide the shell (kill by numeric PID instead); a `kill $(...)` that matches the nohup substring in its own command line does the same.
- Lesson: one Python client (even 8-thread) saturates itself ~5.7k before the server; concurrency claims need ≥2 load processes.

## 2026-09-04 — Fixed the POST RST: `$read_request` loops until headers + Content-Length
- `server.wat`: new `$find_eoh` + `$content_len` (exact-case `Content-Length:` at `0xD096`) + `$read_request` (8K cap; -1 closed, -2 oversize/malformed → 400). `$handle` uses it; rest untouched.
- Proof: 200+200+100 rapid POSTs (3B/1K/5K) all 200, zero resets; 5KB digest matches `hashlib`; full curl matrix green; bench POST row err=0 (was 30/200), GET numbers unchanged (framing loop costs nothing when headers arrive whole).
- Committed (`server.wat` + rebuilt tracked `server.wasm`).

## 2026-09-04 — Benchmark: ~4600 rps, WASM within 0.05 ms of a Python baseline; found a real POST bug
- `python3 /tmp/opencode/bench.py` (500+300+300+200 sequential, 8x100 concurrent) vs release `host-rs` on :8124 and a single-threaded canned-response Python baseline on :8125.
- GET / (570B) sequential: WASM avg 0.21 ms p99 0.43, 4624 rps — baseline avg 0.16 ms p99 0.38, 5990 rps. 404 path fastest (5170 rps). 8-thread: WASM 5756 rps, baseline 5129 rps — serial accept loop, queueing, no errors on GET.
- BUG: `POST /sha256` fails ~20% under rapid fire (`ConnectionResetError`), 0% with 20 ms gaps, any body size. Cause: `server.wat:$handle` does ONE `recv` then answers — headers and body can arrive split across packets, so it hashes a partial body, sends 200, closes mid-send → RST. GETs never trip it (no body). Fix belongs in the app (recv loop until headers + Content-Length), not the harness.
- `python3 -m http.server` refused to stay up in this sandbox; a 20-line single-threaded `baseline.py` (canned 680B) worked fine.

## 2026-09-04 — Restructure around the harness + CLI that explains itself
- Layout: `host-rs/src/{main,manifest,host,net,link,cmds}`, `examples/{hello,pi,server}/`, `libs/{http,sha256}/`, `tools/serve.py` (retired), `AGENTS.md` with repo rules. Examples' `.wasm` tracked as distributables; everything else generated stays ignored.
- Harness CLI: bare `host-rs` prints help (never boots a demo); `run`/`check`/`inspect`/`init`. `inspect` shows a foreign module's imports/exports (the cross-language on-ramp); `init` scaffolds a manifest stub. Manifest paths resolve manifest-dir-first with CWD fallback — runnable from anywhere.
- Fixes: wasmtime 48 has no `ValKind` (`ValType` displays directly, no `PartialEq`); `Caller` sees only the direct caller's exports (host keeps the Memory handle).

## 2026-09-04 — Generic harness: manifest-driven host-rs runs everything
- Rewrote host-rs around TOML manifests (mode/libs/bridges/app); verified same binary runs server (`srv/manifest.toml`, full matrix incl. sha256), pi and hello (`src/*.toml`, exit codes via I32Exit) with zero rebuilds. Lib exports auto-wired from module metadata; shared memory sized from import minima.
- Fixes: `MemoryType::new` takes u32 pages; manifest `memory_pages` is a floor, not the size.
- Learning: harness = load + link + syscalls; everything app-specific lives in the manifest. New ABI shapes (UDP, other bridge arities) extend the host once.

## 2026-09-04 — Python-free: Rust host + crates.io sha2 lib (POST /sha256)
- `rustup target add wasm32-wasip1` (user-approved). `host-rs/` on wasmtime 48 (`p1::add_to_linker_sync`, `build_p1`) replaces `serve.py` — full curl matrix green, zero Python at runtime.
- `lib-sha256/` (sha2 0.10, cdylib) bridged via host memcpy (Rust std keeps own memory); `POST /sha256` matches `sha256sum` on `abc` and a 5 KB blob. lib `parse_request` now returns paths for non-GET too.
- Bugs: Rust `Caller` sees the DIRECT caller's exports (lib has no memory export) → host stores the Memory handle instead + lib re-exports memory; wasmtime 48 `Memory::read/write(store, offset, buf)` arg order; `Instance::exports` yields `Export` structs → wire known exports explicitly.
- Learning: any language emitting core modules plugs in (Rust/C/TinyGo/AS/Zig yes; mainline Go JS-only; npm/PyPI no). Memory strategy is the only real question: share vs bridge-copy.

## 2026-09-04 — Static file server in IR + lib-reuse proof (lib/ + srv/)
- Built `lib/http.wat` (response helpers) + `srv/server.wat` (accept loop, routing, WASI file serving), linked at runtime through host-owned shared memory; `srv/serve.py` provides only TCP syscalls + linking (marked scaffolding; goal stays Python-free via C host or WASI 0.2 sockets).
- Full curl matrix green: 200s with correct MIME, 404, 403×2 traversal, 405, query-strip. Bugs found: preview1 `path_open` needs out-pointer; wasmtime-py needs `access_caller=True` + `ValType`; hand-counted MIME length 25 vs 24 poisoned headers (curl hides NUL — verify raw with `curl -i`).
- `lib/*.wasm`, `srv/*.wasm` git-ignored as build artifacts (only `src/*.wasm` stay tracked as distributables).

## 2026-09-04 — License pulled: GNU AGPLv3 (added via GitHub web)
- `git pull --ff-only` → `f278fd6 Create LICENSE`, clean fast-forward.
- Note: AGPL is strong copyleft + network clause — serving a modified app over HTTP counts as distribution. Fine for open work; revisit (MIT/Apache-2.0) if closed-source distribution is ever wanted.

## 2026-09-04 — native/gen now git-ignored (was committed by mistake)
- Reason: generated files go stale with wasm2c versions and add 84 KB; committing them didn't even remove the wabt dependency (recipients still need `wasm-rt-*.c`). `git rm --cached`, rebuilt from scratch via `native/build.sh` to prove reproducibility, pushed.
- Learning: generated = ignored, always; vendor explicitly if a release bundle needs to be wabt-free.

## 2026-09-04 — Repo live: github.com/renierr/ai-direct-ir (public)
- Steps: `git init -b main`, `.gitignore` (binaries, `*.cwasm`, `*.o`; `gen/` C committed deliberately for distribution), 33 files committed (`fbfaf63`), `gh` login OK but token lacked `createRepository` → user created empty repo on web → `git remote add origin` + `git push -u origin main` succeeded.
- Learning: `gh auth login` tokens may miss repo-creation scope; website-create + CLI-push is the reliable fallback.

## 2026-09-04 — Distributable apps without Docker (portable-C shim)
- Goal: user wants to send apps to others, no Docker.
- Change: ported `native/wasi_shim.c` from POSIX `read`/`write` to stdio (`fread`/`fwrite`+`fflush`, `exit`) — now pure C99, builds with any compiler incl. MSVC/mingw. Rebuilt, re-verified N=1000 identical, invalid→exit 1.
- Options documented in `docs/15-distribute.md`: (1) send 1 KB `.wasm` + wasmtime install line, (2) send portable C bundle (any OS compiles), (3) prebuilt exes — Linux done, Windows needs `mingw-w64-gcc` (not installed; ask first), macOS via recipient-side clang.
- Learning: the C output of wasm2c + a stdio-only shim is the universal artifact — every platform already has a C compiler.

## 2026-09-04 — Ship-with-runtime sketches (Dockerfile + systemd)
- Goal: show production path without wasm2c: runtime + `.wasm` as deploy unit.
- Added: `ship/Dockerfile` (pinned wasmtime 48.0.1, AOT `.cwasm` baked in), `ship/pi-wasi.service` (hardened unit), `docs/14-ship-with-runtime.md` (container/systemd/K8s/serverless + prod checklist).
- Tried `docker build` → daemon socket not accessible from this session (needs group/daemon fix — user-side). Dockerfile untested, review before use.
- Learning: prod story needs no new tech — same two files (runtime + module), plus pinning, least-capability flags, and the existing validate+test gate in CI.

## 2026-09-04 — wasm2c: true native exes, byte-identical to wasmtime
- Goal: standalone binary with no runtime; answer whether it stays memory-safe.
- Needed (all present, zero installs): `wasm2c` + `wasm-rt` from Arch `wabt`; handwritten `native/wasi_shim.c` (3 imports → POSIX) + `native/main_*.c`; `native/build.sh` reproduces all.
- Results: `hello-native` (23 KB) + `pi-native` (23 KB), ELF linked only to libc. N=1000 output byte-identical vs `wasmtime run` (1003 B each); invalid inputs exit 1 in both. Fixed one own bug along the way (`u32` typedefs live in generated headers — mirrored the `WASM_RT_CORE_TYPES_DEFINED` guard in shim).
- Safety: module bugs still trap (RANGE_CHECK on every access, verified in generated C). New TCB = shim (fully range-checked, overflow-safe, hardened flags) + wasm2c codegen + C toolchain. Rule: wasmtime for untrusted/dev, wasm2c for release of own verified modules; keep import surfaces tiny.
- Full analysis: `docs/13-wasm2c-native.md`.

## 2026-09-04 — pi.wat: interactive pi to N digits (Phase 1, AI-written WAT)
- Goal: complex example — prompt for 0..1000, validate, compute pi, print. No floats.
- Program: `src/pi.wat` (~300 lines, AI-written) -> `src/pi.wasm` 1120 bytes via `wat2wasm`, `wasm-tools validate` OK.
- Method: WASI `fd_write` (prompt/stdout/stderr) + `fd_read` (stdin) + `proc_exit`. Hand-rolled `$parse` (leading/trailing spaces OK, `>1000`/junk/empty/EOF -> stderr + exit 1, `N=0` -> `3`). Rabinowitz-Wagon integer spigot, `len=10*(N+2)/3+1`, 1 guard digit, truncated output `3.xxx`.
- Verification (independent algorithm!): Python `decimal` Chudnovsky to 1000 digits vs WASM spigot — **EXACT MATCH at N=100 and N=1000**. Cautionary tale: my remembered pi string (`...3421070679`) was wrong; two independent algorithms agreeing beats memory. Trust the cross-check, not recall.
- Edge cases: `1000` OK (~33ms), `"  42"`/`"7 "`/`"007"` accepted, `"12x"`/`""`/`"-5"`/`1001` rejected with exit 1.
- Learning: AI can write real interactive WAT (I/O, parsing, loops, carry logic) that validates first try and is bit-exact vs independent computation. Phase 1 essentially proven; next is raw-binary emission of same.

## 2026-09-04 — Full toolchain verified, Python emitter == wat2wasm byte-identical
- Goal: verify user-installed toolchain, cross-check AI-direct emitter.
- Toolchain (Omarchy/Arch `extra`):
  - `wasmtime 48.0.1`, `wat2wasm 1.0.41` (wabt), `wasm-opt version 130` (binaryen), `wasm-tools 1.257.1`
- Commands:
  - `wat2wasm src/hello.wat -o /tmp/hello.wat.wasm` -> 152 bytes
  - `cmp src/hello.wasm /tmp/hello.wat.wasm` -> **identical bytes**
  - `wasm-tools validate` both -> OK
  - `wasmtime run` both -> `hello from AI-direct IR`
  - `wasm-opt -O3 src/hello.wasm -o /tmp/hello.opt.wasm` -> still 152 bytes, runs OK (already optimal)
  - `wasm-tools print src/hello.wasm` -> round-trips to same module (2 types, 1 import, 1 memory, 2 exports, 1 func, 1 data)
- Learning: Python direct-binary emitter is correct — bit-for-bit equal to `wat2wasm`. This is the key proof for AI-direct IR: AI can emit binary without going through text. `wasm-opt` no-op on tiny module as expected. Phase 0 complete.
- Rule added: agent must ask before `sudo/pacman/yay/paru` (saved in `~/.config/opencode/opencode.json` permission section).

## 2026-09-04 — hello.wasm runs on wasmtime (no wat2wasm needed)
- Goal: give user a testable `hello.wasm` with only `wasmtime` installed.
- Commands:
  - `python3 src/build_hello.py` (emits 152-byte WASM directly, no toolchain)
  - `wasmtime run src/hello.wasm` -> `hello from AI-direct IR`
- Output: works on wasmtime 48.0.1. Module imports `wasi_snapshot_preview1.fd_write`, 1 memory page, exports `_start`, writes 24 bytes from offset 8 via iovec at 0.
- Learning: proves AI-direct IR loop: Python builder (standing in for AI) -> raw `.wasm` -> `wasmtime run`. No `wat2wasm` required for binary emission. Kept `src/hello.wat` as human-readable mirror of same module.
- Toolchain status (Omarchy/Arch): `wasmtime 48.0.1` OK. Missing `wat2wasm` (pkg `wabt`), `wasm-opt` (pkg `binaryen`), `wasm-tools` (pkg `wasm-tools`) — all in `extra` repo. Install: `sudo pacman -S wabt binaryen wasm-tools`. `sudo` needs interactive terminal, so user runs it manually.

## 2026-09-04 — Init
- Goal: start docs in empty `wasm-demo/`.
- Commands: `ls -la`, created `README.md`, `docs/01-05`.
- Output: empty folder confirmed.
- Learning: docs skeleton ready. Next: install toolchain versions (`wasmtime --version`, `wat2wasm --version`, `wasm-tools --version`, `wasm-opt --version`) and record here.
