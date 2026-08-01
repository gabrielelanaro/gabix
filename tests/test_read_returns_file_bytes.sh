#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST read returns file bytes ==="
prepare_test_disk tests/fixtures/read_file.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-read-file.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
   && grep -Fq "[read-file] expected bytes returned" "$out"; then
    echo "[read-file] PASS"
    rm "$out"
    exit 0
fi

echo "[read-file] FAIL; serial log left at $out" >&2
exit 1
