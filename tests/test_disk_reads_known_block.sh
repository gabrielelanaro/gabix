#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST disk reads known block ==="
prepare_test_disk tests/fixtures/disk_known.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-block.XXXXXX)
boot_qemu_no_hostfwd "$out" || true
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
    && grep -Fq "[disk] known data returned" "$out"; then
    echo "[disk] PASS"
    rm "$out"
    exit 0
fi

echo "[disk] FAIL; serial log left at $out" >&2
exit 1
