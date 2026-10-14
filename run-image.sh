#!/bin/bash
# Boot an already-built disk image (e.g. the one produced by make-usb-image.sh)
# in QEMU, without rebuilding the kernel. Extra arguments are forwarded to QEMU.
#   ./run-image.sh                        # boots ./rust_os.img
#   ./run-image.sh path/to/other.img      # boots a specific image
#   ./run-image.sh rust_os.img -display none
set -euo pipefail
cd "$(dirname "$0")"

IMG="rust_os.img"
if [ $# -gt 0 ] && [ -f "$1" ]; then
    IMG="$1"
    shift
fi

if [ ! -f "$IMG" ]; then
    echo "error: image '$IMG' not found (run ./make-usb-image.sh first)" >&2
    exit 1
fi

exec qemu-system-x86_64 \
    -drive format=raw,file="$IMG" \
    -serial stdio \
    -netdev user,id=net0,hostfwd=tcp::8080-:80 \
    -device e1000,netdev=net0 \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    --no-reboot \
    "$@"
