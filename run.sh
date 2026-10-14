#!/bin/bash
# Build and boot the kernel in QEMU. Extra arguments are forwarded to QEMU, e.g.:
#   ./run.sh -display none          # headless, serial output on stdout
set -euo pipefail
cd "$(dirname "$0")"
export RUSTUP_TOOLCHAIN=nightly-2026-10-13
cargo run -- "$@"
