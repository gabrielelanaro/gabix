#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST DHCP configures network ==="
prepare_test_disk tests/fixtures/dhcp.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-dhcp.XXXXXX)
boot_qemu_no_hostfwd "$out" || true
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code"; then
    echo "[dhcp] PASS"
    rm "$out"
    exit 0
fi

echo "[dhcp] FAIL; serial log left at $out" >&2
exit 1
