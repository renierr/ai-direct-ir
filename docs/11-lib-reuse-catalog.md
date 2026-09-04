# Reusing the World's Libraries from AI-Generated WASM (general)

Question (2026-09-04): not just HTTP — can we use ALL common proven libs (DB, crypto, images, JSON...)? Answer: yes. The pools are large; the trick is the consumption mechanism. AI imports, never reimplements.

## The pools (all compilable to WASM today)

- **C/C++ (biggest pool):** `sqlite3.c` amalgamation (sqlite.org ships official WASM builds), `stb_image`/`stb_truetype` single-headers, `miniz`/`zlib`, `cJSON`, `mbedTLS`/`wolfSSL`, `pcre2`, `libpng`, `mongoose`/`civetweb`. Build with `clang --target=wasm32-wasi` + wasi-sdk sysroot.
- **Rust crates:** `serde_json`, `regex`, `sha2`/RustCrypto, `image`, `http` (no-I/O parsing), `url`, `uuid`. Build with `cargo --target wasm32-wasip2`.
- **Other languages via components:** Python (`componentize-py`), JS (`componentize-js`) — for glue/configuration logic, not hot paths.
- **Registries:** `crates.io` (+ `wasm32-wasip2` gating), `wapm.io`, conan/vcpkg wasm triplets, Bytecode Alliance component proposals.

## How an AI-generated module consumes them (pick ONE pattern)

1. **Component composition (recommended):** lib is prebuilt as a component exposing a WIT interface (`sqlite:query(sql)->rows`, `hash:sha256(bytes)->bytes`). AI emits a plain module that only `import`s that interface. `wac plug lib.wasm ours.wasm -o app.wasm`. AI never touches linking — WIT is the ABI.
2. **Static link via `wasm-ld`:** lib as `.a`/`.o` (wasm objects) + AI emits relocatable object. Powerful but demands relocations/symbol tables from the AI — hardest output format, avoid for now.
3. **WIT standard interfaces (portability layer):** `wasi:keyvalue`, `wasi:blobstore`, `wasi:config`, `wasi:http`, `wasi:clocks`, `wasi:random` — code against the interface, swap backends (Spin, wasmtime, cloud) without rebuilding logic. Prefer where a standard exists.
4. **Host injection (escape hatch):** host maps `env.sha256` etc. to native libs. Fastest for experiments, but artifact isn't self-contained — use only to unblock.

## Worked examples (what "use X" concretely means)

- Need a DB? Don't write storage in WAT — reuse `sqlite3.c` compiled to a `sqlite` component; AI emits SQL strings + row handling.
- Need images? `stb_image` component: `decode(bytes)->pixels`; AI does layout/serve logic.
- Need TLS/HTTPS? `mbedTLS` or Rust `rustls` component below the HTTP layer.
- Need JSON? `serde_json`/`cJSON` component: `parse/emit`; AI handles schema.

## Honest caveats

- Threads (`wasi-threads`), SIMD, GC-proposal coverage vary by runtime — pin `wasmtime` version, test per lib.
- No dynamic-linking culture: everything static → bigger binaries, duplicate deps across components (mitigate via shared component deps / `wac`).
- Async runtimes (tokio) on WASM are weak — prefer sync/event-callback lib APIs (`http` parsing, mongoose events).
- POSIX surface gaps: `fork`, signals, mmap don't exist — libs needing them need patches/shims.
- Supply chain: pin versions + hashes of every prebuilt component; record in lablog.

## Project workflow going forward

Keep a `libs/` catalog: one dir per proven component (`libs/sqlite/`, `libs/sha256/`, `libs/http-server/`...) with pinned `.wasm` + its `.wit` + provenance (source, version, build cmd). AI's job per app: write `app.wit` imports + logic module; build script composes. First catalog entry should be tiny and dependency-free (e.g. `sha256`) to prove the composition loop before attempting `sqlite`.
