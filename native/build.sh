#!/usr/bin/env bash
# Build true standalone native exes from our .wasm modules via wasm2c.
# Needs: wabt (wasm2c), clang. No installs, no network.
set -euo pipefail
cd "$(dirname "$0")"

RT=/usr/share/wabt/wasm2c
CFLAGS="-O2 -Wall -Wextra -Wno-unused-function -Wno-unused-parameter -fstack-protector-strong -D_FORTIFY_SOURCE=2 -Igen -I."

echo "== wasm2c: .wasm -> C =="
mkdir -p gen
wasm2c ../src/hello.wasm -o gen/hello.c
wasm2c ../src/pi.wasm -o gen/pi.c

echo "== clang: C -> native =="
clang $CFLAGS gen/hello.c wasi_shim.c main_hello.c \
  $RT/wasm-rt-impl.c $RT/wasm-rt-mem-impl.c -o hello-native
clang $CFLAGS gen/pi.c wasi_shim.c main_pi.c \
  $RT/wasm-rt-impl.c $RT/wasm-rt-mem-impl.c -o pi-native

echo "== smoke test =="
./hello-native
echo 25 | ./pi-native
ls -l hello-native pi-native
