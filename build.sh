#!/bin/bash
# Build the kernel and assemble the bootable BIOS disk image.
set -euo pipefail
cd "$(dirname "$0")"
export RUSTUP_TOOLCHAIN=nightly-2026-10-13
cargo build "$@"
