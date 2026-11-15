#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST ARM virt isolates processes ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" virt | tail -1)
out=$(mktemp /tmp/mini-os-test-arm-virt-task.XXXXXX)
qemu=${QEMU_SYSTEM_ARM:-/tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm}

QEMU_MODULE_DIR=/tmp/gabix-qemu-arm/root/usr/lib/qemu \
"$TIMEOUT_CMD" "$QEMU_TIMEOUT" "$qemu" \
    -M virt \
    -cpu cortex-a15 \
    -smp 1 \
    -m 128M \
    -kernel "$kernel" \
    -semihosting-config enable=on,target=native \
    -display none \
    -audio none \
    -serial "file:$out" \
    -no-reboot
exit_code=$?

if [ "$exit_code" -eq 0 ] && grep -Fqx "gabix arm isolated userspace processes passed" "$out" && grep -Fqx "gabix arm user faults contained; peer process survived" "$out"; then
    echo "[arm-virt-isolation] PASS"
    rm "$out"
    exit 0
fi

echo "[arm-virt-isolation] FAIL; serial log left at $out" >&2
exit 1
