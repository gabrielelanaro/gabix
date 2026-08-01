#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST fork shares file cursor ==="
prepare_test_disk tests/fixtures/inherit_wait.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-fork-cursor.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
   && grep -Fq "[inherit] child read shared fd" "$out" \
   && grep -Fq "[inherit] parent saw child advance cursor" "$out"; then
    echo "[fork-cursor] PASS"
    rm "$out"
    exit 0
fi

echo "[fork-cursor] FAIL; serial log left at $out" >&2
exit 1
