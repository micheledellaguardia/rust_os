#!/bin/bash
# Build the kernel and copy the bootable BIOS disk image it produces to a
# plain file, ready to be written to a USB stick for booting on real
# hardware (legacy/BIOS boot, not UEFI).
#
# This is a plain file COPY ONLY: it never touches a block device by itself.
# To actually flash it, run the printed `dd` command yourself after checking
# the target device name very carefully — dd to the wrong device destroys
# data with no warning.
#
# Usage: ./make-usb-image.sh [output.img]
set -euo pipefail
cd "$(dirname "$0")"
export RUSTUP_TOOLCHAIN=nightly-2026-10-13

OUT_IMG="${1:-rust_os.img}"

echo "Building kernel and BIOS disk image..."
cargo build

# `cargo build` regenerates target/debug/build/os-<hash>/out/bios.img; the hash
# can change between invocations, so pick whichever copy was written most
# recently rather than hardcoding a path.
BIOS_IMG=$(find target/debug/build/os-*/out/bios.img 2>/dev/null | xargs -r ls -t | head -n1)
if [ -z "$BIOS_IMG" ]; then
    echo "error: could not find bios.img under target/debug/build/os-*/out/ (did the build succeed?)" >&2
    exit 1
fi

cp "$BIOS_IMG" "$OUT_IMG"
echo "Created $OUT_IMG ($(du -h "$OUT_IMG" | cut -f1))"
echo
echo "Test it in QEMU first:"
echo "  qemu-system-x86_64 -drive format=raw,file=$OUT_IMG -serial stdio"
echo
echo "To boot on real hardware (BIOS/legacy only, not UEFI), write it to a USB"
echo "stick. Find the device with 'lsblk' first (e.g. /dev/sdX), make sure it's"
echo "the right one, then:"
echo "  sudo dd if=$OUT_IMG of=/dev/sdX bs=4M status=progress conv=fsync"
echo "WARNING: dd overwrites the ENTIRE target device with no further warning."
