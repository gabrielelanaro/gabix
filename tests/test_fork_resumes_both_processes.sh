#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST fork resumes both processes ==="
prepare_test_disk tests/fixtures/fork_smoke.rs
build_kernel

out=$(mktemp /tmp/mini-os-test-fork-processes.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

if qemu_exit_was_success "$exit_code" \
   && grep -Fq "[fork_smoke] parent" "$out" \
   && grep -Fq "[fork_smoke] child" "$out"; then
    echo "[fork-processes] PASS"
    rm "$out"
    exit 0
fi

echo "[fork-processes] FAIL; serial log left at $out" >&2
exit 1
