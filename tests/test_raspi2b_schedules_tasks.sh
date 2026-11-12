#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST Raspberry Pi 2B schedules tasks ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" raspi2b | tail -1)
out=$(mktemp /tmp/mini-os-test-raspi2b.XXXXXX)
qemu=${QEMU_SYSTEM_ARM:-/tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm}

QEMU_MODULE_DIR=/tmp/gabix-qemu-arm/root/usr/lib/qemu \
"$TIMEOUT_CMD" "$QEMU_TIMEOUT" "$qemu" \
    -M raspi2b \
    -kernel "$kernel" \
    -semihosting-config enable=on,target=native \
    -display none \
    -audio none \
    -serial "file:$out" \
    -no-reboot
exit_code=$?

if [ "$exit_code" -eq 0 ] && grep -Fqx "gabix arm timer preempted two non-yielding tasks" "$out" && grep -Fqx "gabix arm scheduler tasks finished" "$out" && grep -Fqx "gabix arm scheduler trace 10 20 11 21 12 22" "$out"; then
    echo "[raspi2b-scheduler] PASS"
    rm "$out"
    exit 0
fi

echo "[raspi2b-scheduler] FAIL; serial log left at $out" >&2
exit 1
