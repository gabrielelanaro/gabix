#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST exit returns status ==="
prepare_test_disk tests/fixtures/inherit_wait.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-exit-status.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
   && grep -Fq "[inherit] wait specific status=7" "$out"; then
    echo "[exit-status] PASS"
    rm "$out"
    exit 0
fi

echo "[exit-status] FAIL; serial log left at $out" >&2
exit 1
