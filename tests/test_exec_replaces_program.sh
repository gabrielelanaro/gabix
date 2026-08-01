#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST exec replaces program ==="
prepare_test_disk tests/fixtures/exec_replace.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-exec-replace.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
   && grep -Fq "[exec-test] before exec" "$out" \
   && grep -Fq "hello, world" "$out" \
   && ! grep -Fq "[exec-test] exec returned" "$out"; then
    echo "[exec-replace] PASS"
    rm "$out"
    exit 0
fi

echo "[exec-replace] FAIL; serial log left at $out" >&2
exit 1
