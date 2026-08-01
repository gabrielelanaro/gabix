#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST Raspberry Pi 2B reads partition ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" raspi2b | tail -1)
root=$(mktemp /tmp/mini-os-raspi2b-root.XXXXXX.img)
disk=$(mktemp /tmp/mini-os-raspi2b-partition.XXXXXX.img)
out=$(mktemp /tmp/mini-os-test-raspi2b-partition.XXXXXX)
qemu=${QEMU_SYSTEM_ARM:-/tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm}

(cd "$REPO_ROOT" && sh scripts/make-arm-disk.sh tests/fixtures/arm_syscall.rs "$root") >/dev/null
python3 "$REPO_ROOT/scripts/assemble-mbr.py" "$disk" "0x83:2048:$root"

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

rm -f "$root" "$disk"

if [ "$exit_code" -eq 0 ] &&
    grep -Fqx "gabix raspi2b file read" "$out" &&
    grep -Fqx "gabix arm userspace wrote" "$out"; then
    echo "[raspi2b-partition] PASS"
    rm "$out"
    exit 0
fi

echo "[raspi2b-partition] FAIL; serial log left at $out" >&2
exit 1
