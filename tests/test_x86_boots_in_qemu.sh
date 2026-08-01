#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST x86 boots in QEMU ==="
prepare_test_disk tests/fixtures/hello.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-x86-boot.XXXXXX)
boot_qemu_no_hostfwd "$out" || true
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code"; then
    echo "[x86-boot] PASS"
    rm "$out"
    exit 0
fi

echo "[x86-boot] FAIL; serial log left at $out" >&2
exit 1
