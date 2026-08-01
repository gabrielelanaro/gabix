#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST boot starts userspace ==="
prepare_test_disk tests/fixtures/hello.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-boot-userspace.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
   && serial_contains "$out" "hello, world"; then
    echo "[boot-userspace] PASS"
    rm "$out"
    exit 0
fi

echo "[boot-userspace] FAIL; serial log left at $out" >&2
exit 1
