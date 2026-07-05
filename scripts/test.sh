#!/bin/sh
# Boot mini-os in QEMU under the test-mode feature, with the isa-debug-exit
# device attached. Captures QEMU's process exit code and translates the
# isa-debug-exit success code (33) into a Unix 0 so cargo test / shell
# scripts see a clean pass.
#
# Usage:
#   ./scripts/test.sh
#
# QEMU exit code translation (from isa-debug-exit formula `(byte << 1) | 1`):
#   33 (0x10 written) → success → return 0
#   anything else     → failure → return 1
#
# See src/qemu_exit.rs for the kernel-side helper that writes the byte.

# Don't use `set -e` here — we need to capture QEMU's exit code, which is
# non-zero even on success (it's 33 for success, see above).

DISK_IMG=disk.img
KERNEL=target/x86_64-mini-os/debug/bootimage-mini-os.bin

if [ ! -f "$DISK_IMG" ]; then
    echo "Run ./scripts/make-disk.sh first to create $DISK_IMG" >&2
    exit 1
fi

# Build the bootimage with the test-mode feature so panics call exit_qemu.
echo "[test-runner] building with --features test-mode"
cargo bootimage --features test-mode 2>&1 | tail -3

# Boot QEMU with isa-debug-exit attached. Otherwise identical to run.sh.
qemu-system-x86_64 \
    -drive format=raw,file="$KERNEL" \
    -drive file="$DISK_IMG",format=raw,if=none,id=drv0 \
    -device virtio-blk-pci,drive=drv0 \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -display none \
    -serial stdio \
    -no-reboot

qemu_exit=$?

if [ "$qemu_exit" -eq 33 ]; then
    echo "[test-runner] QEMU exit $qemu_exit → PASS"
    exit 0
else
    echo "[test-runner] QEMU exit $qemu_exit → FAIL" >&2
    exit 1
fi
