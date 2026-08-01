#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST invalid pointer returns error ==="
prepare_test_disk tests/fixtures/invalid_pointer.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-invalid-pointer.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
   && grep -Fq "[invalid-pointer] error returned" "$out"; then
    echo "[invalid-pointer] PASS"
    rm "$out"
    exit 0
fi

echo "[invalid-pointer] FAIL; serial log left at $out" >&2
exit 1
