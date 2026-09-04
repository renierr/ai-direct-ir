#!/usr/bin/env bash
# Build the host-rs harness executable. The current platform is the default.
set -euo pipefail

usage() {
  printf '%s\n' \
    'Usage: ./build.sh [--target <triple>]' \
    '' \
    'Builds the optimized host-rs harness executable with Cargo.' \
    '' \
    'Examples:' \
    '  ./build.sh' \
    '  ./build.sh --target x86_64-pc-windows-gnu' \
    '' \
    'Artifacts:' \
    '  dist/host-rs' \
    '  dist/<triple>/host-rs[.exe]' \
    '' \
    'Cross-compilation requires that the requested Rust target and its native linker' \
    'are already installed. This script never installs them. For Windows GNU builds' \
    'from Linux, install the Rust target and a MinGW-w64 linker first, then run:' \
    '  ./build.sh --target x86_64-pc-windows-gnu' \
    '' \
    'MSVC Windows builds require Microsoft'\''s MSVC linker/toolchain and are normally' \
    'built on Windows.'
}

target=""
case "${1:-}" in
  "") ;;
  --target)
    target="${2:-}"
    if [[ -z "$target" || $# -ne 2 ]]; then
      usage >&2
      exit 2
    fi
    ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

mkdir -p dist
if [[ -n "$target" ]]; then
  if ! rustup target list --installed | command grep -Fxq "$target"; then
    printf 'Rust target %q is not installed. Install it explicitly, then rerun this command.\n' "$target" >&2
    exit 1
  fi
  cargo build --manifest-path host-rs/Cargo.toml --release --target "$target"
  mkdir -p "dist/$target"
  executable="host-rs"
  if [[ "$target" == *windows* ]]; then
    executable="host-rs.exe"
  fi
  cp "host-rs/target/$target/release/$executable" "dist/$target/$executable"
  printf 'built dist/%s/%s\n' "$target" "$executable"
else
  cargo build --manifest-path host-rs/Cargo.toml --release
  cp host-rs/target/release/host-rs dist/host-rs
  printf 'built dist/host-rs\n'
fi
