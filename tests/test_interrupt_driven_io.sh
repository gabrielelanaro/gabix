#!/bin/sh
# E2E proof for Lesson 28: APIC + virtio-blk MSI-X and successful disk I/O.

. "$(dirname "$0")/common.sh"

echo "=== TEST interrupt-driven virtio-blk I/O ==="
build_kernel "test-mode"

out=$(mktemp /tmp/mini-os-test-interrupt-io.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

echo "[interrupt-io] QEMU exit code: $exit_code"
grep -E "\[apic\]|MSI-X|ext2 mounted|hello, world" "$out" | tail -12

if qemu_exit_was_success "$exit_code" \
   && serial_contains "$out" "\[apic\] enabled" \
   && serial_contains "$out" "MSI-X queue 0 -> table 0 -> vector 0x21" \
   && serial_contains "$out" "first disk completion delivered by MSI-X" \
   && serial_contains "$out" "ext2 mounted" \
   && serial_contains "$out" "hello, world"; then
    echo "[interrupt-io] PASS"
    rm "$out"
    exit 0
else
    echo "[interrupt-io] FAIL — serial log left at $out" >&2
    exit 1
fi
