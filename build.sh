#!/bin/bash
# Build the kernel and assemble the bootable BIOS disk image.
set -euo pipefail
cd "$(dirname "$0")"
export RUSTUP_TOOLCHAIN=nightly-2025-02-01
cargo build "$@"
