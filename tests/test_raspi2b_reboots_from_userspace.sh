#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST Raspberry Pi 2B reboots from userspace ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" raspi2b-lifecycle | tail -1)
disk=$(mktemp /tmp/mini-os-raspi2b-reboot.XXXXXX.img)
out=$(mktemp /tmp/mini-os-test-raspi2b-reboot.XXXXXX)
qemu=${QEMU_SYSTEM_ARM:-qemu-system-arm}

(cd "$REPO_ROOT" && sh scripts/make-arm-disk.sh tests/fixtures/arm_reboot.rs "$disk") >/dev/null

QEMU_MODULE_DIR=${QEMU_MODULE_DIR:-/opt/homebrew/lib/qemu} \
"$TIMEOUT_CMD" 5 "$qemu" \
    -M raspi2b \
    -kernel "$kernel" \
    -drive file="$disk",format=raw,if=sd \
    -semihosting-config enable=on,target=native \
    -display none \
    -audio none \
    -serial "file:$out" \
    -no-reboot
exit_code=$?

if [ "$exit_code" -eq 0 ] &&
    grep -Fqx "gabix arm userspace rebooting" "$out"; then
    echo "[raspi2b-reboot] PASS"
    rm "$out" "$disk"
    exit 0
fi

echo "[raspi2b-reboot] FAIL; serial log left at $out" >&2
echo "[raspi2b-reboot] disk left at $disk" >&2
exit 1
