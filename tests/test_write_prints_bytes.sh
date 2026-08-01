#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST write prints bytes ==="
prepare_test_disk tests/fixtures/hello.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-write.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
   && [ "$(grep -c '^hello, world' "$out")" -eq 1 ]; then
    echo "[write] PASS"
    rm "$out"
    exit 0
fi

echo "[write] FAIL; serial log left at $out" >&2
exit 1
