#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST Raspberry Pi 2B reads block ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" raspi2b | tail -1)
disk=$(mktemp /tmp/mini-os-raspi2b-block.XXXXXX.img)
out=$(mktemp /tmp/mini-os-test-raspi2b-block.XXXXXX)
qemu=${QEMU_SYSTEM_ARM:-/tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm}

truncate -s 1M "$disk"
printf 'GABIXPI2' | dd of="$disk" bs=1 seek=512 conv=notrunc 2>/dev/null

QEMU_MODULE_DIR=/tmp/gabix-qemu-arm/root/usr/lib/qemu \
"$TIMEOUT_CMD" "$QEMU_TIMEOUT" "$qemu" \
    -M raspi2b \
    -kernel "$kernel" \
    -drive file="$disk",format=raw,if=sd \
    -semihosting-config enable=on,target=native \
    -display none \
    -audio none \
    -serial "file:$out" \
    -no-reboot
exit_code=$?

rm -f "$disk"

if [ "$exit_code" -eq 0 ] && grep -Fqx "gabix raspi2b block read" "$out"; then
    echo "[raspi2b-block] PASS"
    rm "$out"
    exit 0
fi

echo "[raspi2b-block] FAIL; serial log left at $out" >&2
exit 1
