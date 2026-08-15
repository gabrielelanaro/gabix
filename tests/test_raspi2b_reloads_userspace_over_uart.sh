#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST Raspberry Pi 2B reloads userspace over UART ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" raspi2b-uart-user | tail -1)
user_elf=$(mktemp /tmp/mini-os-arm-uart-user.XXXXXX)
out=$(mktemp /tmp/mini-os-test-raspi2b-uart-user.XXXXXX)
qemu=${QEMU_SYSTEM_ARM:-qemu-system-arm}

sh "$REPO_ROOT/scripts/build-arm-user.sh" \
    tests/fixtures/arm_syscall.rs "$user_elf" >/dev/null

QEMU_MODULE_DIR=${QEMU_MODULE_DIR:-/opt/homebrew/lib/qemu} \
python3 "$TESTS_DIR/support/raspi2b_uart_user_upload.py" \
    "$qemu" "$kernel" "$user_elf" "$out"
exit_code=$?

rm -f "$user_elf"

if [ "$exit_code" -eq 0 ]; then
    echo "[raspi2b-uart-user] PASS"
    rm -f "$out"
    exit 0
fi

echo "[raspi2b-uart-user] FAIL; serial log left at $out" >&2
exit 1
