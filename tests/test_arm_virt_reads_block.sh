#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST ARM virt reads block ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" virt | tail -1)
disk=$(mktemp /tmp/mini-os-arm-block.XXXXXX.img)
out=$(mktemp /tmp/mini-os-test-arm-virt-block.XXXXXX)
qemu=${QEMU_SYSTEM_ARM:-/tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm}

truncate -s 1M "$disk"
printf 'GABIXARM' | dd of="$disk" bs=1 seek=512 conv=notrunc 2>/dev/null

QEMU_MODULE_DIR=/tmp/gabix-qemu-arm/root/usr/lib/qemu \
"$TIMEOUT_CMD" "$QEMU_TIMEOUT" "$qemu" \
    -M virt \
    -cpu cortex-a15 \
    -smp 1 \
    -m 128M \
    -kernel "$kernel" \
    -drive file="$disk",format=raw,if=none,id=drv0 \
    -device virtio-blk-device,drive=drv0 \
    -global virtio-mmio.force-legacy=false \
    -semihosting-config enable=on,target=native \
    -display none \
    -audio none \
    -serial "file:$out" \
    -no-reboot
exit_code=$?

rm -f "$disk"

if [ "$exit_code" -eq 0 ] && grep -Fqx "gabix arm virt block read" "$out"; then
    echo "[arm-virt-block] PASS"
    rm "$out"
    exit 0
fi

echo "[arm-virt-block] FAIL; serial log left at $out" >&2
exit 1
