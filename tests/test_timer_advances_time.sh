#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST timer advances time ==="
prepare_test_disk tests/fixtures/timer.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-timer.XXXXXX)
boot_qemu_no_hostfwd "$out" || true
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
    && grep -Fq "[timer] monotonic time advanced" "$out"; then
    echo "[timer] PASS"
    rm "$out"
    exit 0
fi

echo "[timer] FAIL; serial log left at $out" >&2
exit 1
