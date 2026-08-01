#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST Raspberry Pi 2B returns to kernel ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" raspi2b-lifecycle | tail -1)
disk=$(mktemp /tmp/mini-os-raspi2b-lifecycle.XXXXXX.img)
out=$(mktemp /tmp/mini-os-test-raspi2b-lifecycle.XXXXXX)
qemu=${QEMU_SYSTEM_ARM:-/tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm}

(cd "$REPO_ROOT" && sh scripts/make-arm-disk.sh tests/fixtures/arm_syscall.rs "$disk") >/dev/null

QEMU_MODULE_DIR=/tmp/gabix-qemu-arm/root/usr/lib/qemu \
"$TIMEOUT_CMD" 3 "$qemu" \
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

if [ "$exit_code" -eq 124 ] &&
    grep -Fqx "gabix arm userspace wrote" "$out" &&
    grep -Fqx "gabix raspi2b userspace exited" "$out"; then
    echo "[raspi2b-lifecycle] PASS"
    rm "$out"
    exit 0
fi

echo "[raspi2b-lifecycle] FAIL; serial log left at $out" >&2
exit 1
