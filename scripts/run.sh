#!/bin/sh
# Launch mini-os in QEMU with the virtio-blk disk attached.
#
# Builds the bootimage first (cargo bootimage), then launches QEMU with two
# drives: the kernel bootimage and the raw disk image. Serial output goes to
# stdout. Ctrl-C to stop, or the kernel halts and you kill QEMU manually.
#
# Usage:
#   ./scripts/run.sh             # interactive, blocks until you kill it
#
# For automated testing with a fixed timeout, wrap with shell tooling:
#   (./scripts/run.sh &) ; sleep 10 ; pkill -f qemu-system

set -e

DISK_IMG=disk.img
KERNEL=target/x86_64-mini-os/debug/bootimage-mini-os.bin

if [ ! -f "$DISK_IMG" ]; then
    echo "Run ./scripts/make-disk.sh first to create $DISK_IMG" >&2
    exit 1
fi

# Build the bootimage fresh
cargo bootimage 2>&1 | tail -3

# Launch QEMU with both drives
#   - First -drive: the kernel bootimage (bootloader sees this as the boot disk)
#   - Second -drive + -device: the virtio-blk-pci disk (kernel discovers via PCI)
exec qemu-system-x86_64 \
    -drive format=raw,file="$KERNEL" \
    -drive file="$DISK_IMG",format=raw,if=none,id=drv0 \
    -device virtio-blk-pci,drive=drv0 \
    -display none \
    -serial stdio \
    -no-reboot
