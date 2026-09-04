#!/usr/bin/env bash
# Build the native host executable. The current platform is the default.
set -euo pipefail

usage() {
  printf '%s\n' \
    'Usage: ./build.sh [--target <triple>]' \
    '' \
    'Builds an optimized host-rs executable with Cargo.' \
    '' \
    'Examples:' \
    '  ./build.sh' \
    '  ./build.sh --target x86_64-pc-windows-gnu' \
    '' \
    'Artifacts:' \
    '  target/release/host-rs' \
    '  target/<triple>/release/host-rs[.exe]' \
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

if [[ -n "$target" ]]; then
  if ! rustup target list --installed | command grep -Fxq "$target"; then
    printf 'Rust target %q is not installed. Install it explicitly, then rerun this command.\n' "$target" >&2
    exit 1
  fi
  cargo build --release --target "$target"
  printf 'built target/%s/release/host-rs\n' "$target"
else
  cargo build --release
  printf 'built target/release/host-rs\n'
fi
