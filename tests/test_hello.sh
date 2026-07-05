#!/bin/sh
# E2E test: exec /hello.elf, verify "hello, world" appears in serial output.
#
# This is the simplest E2E test — it just confirms the mission-landing path
# still works: kernel boots, all init assertions pass, /hello.elf loads and
# runs, sys_write fires, sys_exit fires, scheduler reaches clean shutdown.
#
# Pass: QEMU exits 33 (isa-debug-exit success) AND "hello, world" is in serial.
# Fail: anything else (panic, hang, missing output).

. "$(dirname "$0")/common.sh"

echo "=== TEST hello (mission landing) ==="

build_kernel "test-mode"

out=$(mktemp /tmp/mini-os-test-hello.XXXXXX)
boot_qemu "$out"
exit_code=$QEMU_EXIT_CODE

echo "[hello] QEMU exit code: $exit_code"
echo "[hello] === tail of serial output ==="
tail -8 "$out"

if qemu_exit_was_success "$exit_code" \
   && serial_contains "$out" "hello, world"; then
    echo "[hello] PASS"
    rm "$out"
    exit 0
else
    echo "[hello] FAIL — serial log left at $out" >&2
    exit 1
fi
