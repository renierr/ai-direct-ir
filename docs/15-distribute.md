# Sending the App to Others (no Docker)

Three options, cheapest first. Verified 2026-09-04 on Omarchy Linux.

## Option 1 — Send the `.wasm` (1 KB, every OS)

`src/pi.wasm` (1120 bytes!) runs on any OS with a runtime. Recipient installs once:

```bash
# Linux/macOS/Windows (powershell): https://wasmtime.dev -> install, then:
wasmtime run pi.wasm
```

Smallest thing you can send. Downside: recipient installs wasmtime first.

## Option 2 — Send portable C (compiles to native anywhere)

`native/gen/*.c` (from wasm2c) + `native/wasi_shim.c` + `native/main_pi.c` is plain C99 with **no POSIX left** — the shim now uses only `fread`/`fwrite`/`exit`, so it builds with MSVC, mingw, clang, gcc, on Windows/Mac/Linux:

```bash
# any OS, any C compiler, e.g.:
cc -O2 -Igen -I. gen/pi.c wasi_shim.c main_pi.c \
  wasm-rt-impl.c wasm-rt-mem-impl.c -o pi-native
```

(`wasm-rt-*.c` come with every `wabt` install.) This is the most robust "source distribution": no toolchain of ours required on their side. Regenerate `gen/` with `native/build.sh` before zipping (it's git-ignored as a build artifact).

## Option 3 — Send prebuilt exes (nicest UX, per-OS work)

- Linux x86-64: **done** — `native/pi-native`, `native/hello-native` (23 KB, libc only).
- Windows `.exe`: cross-compile the same C with mingw (`x86_64-w64-mingw32-gcc`, package `mingw-w64-gcc`) → `pi-native.exe`. Toolchain NOT installed here (checked); needs `sudo pacman -S mingw-w64-gcc` — ask user first per rule.
- macOS: no practical cross-compile from Linux for signed binaries; they build Option 2 with Xcode clang in one command.

## Recommendation

Default to Option 1 for technical recipients (one install, always current), Option 3 Linux exe for non-technical Linux users (works now), Option 2 bundle as the universal fallback that covers Windows/Mac without you owning those machines.
