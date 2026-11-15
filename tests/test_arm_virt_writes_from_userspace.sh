#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST ARM virt writes from userspace ==="
kernel=$(sh "$REPO_ROOT/scripts/build-arm.sh" virt | tail -1)
fixture_elf=$(mktemp /tmp/mini-os-arm-user.XXXXXX.elf)
fixture_bin=$(mktemp /tmp/mini-os-arm-user.XXXXXX.bin)
out=$(mktemp /tmp/mini-os-test-arm-virt-user.XXXXXX)
qemu=${QEMU_SYSTEM_ARM:-/tmp/gabix-qemu-arm/root/usr/bin/qemu-system-arm}
rust_sysroot=$(rustc --print sysroot)
rust_host=$(rustc -vV | sed -n 's/^host: //p')
llvm_objcopy="$rust_sysroot/lib/rustlib/$rust_host/bin/llvm-objcopy"

rustc \
    --target armv7a-none-eabihf \
    --edition 2021 \
    -C panic=abort \
    -C relocation-model=static \
    -C link-arg=-T"$REPO_ROOT/tests/fixtures/armv7-user.ld" \
    -C link-arg=-z \
    -C link-arg=max-page-size=0x1000 \
    -o "$fixture_elf" \
    "$REPO_ROOT/tests/fixtures/arm_syscall.rs"
"$llvm_objcopy" -O binary "$fixture_elf" "$fixture_bin"

QEMU_MODULE_DIR=/tmp/gabix-qemu-arm/root/usr/lib/qemu \
"$TIMEOUT_CMD" "$QEMU_TIMEOUT" "$qemu" \
    -M virt \
    -cpu cortex-a15 \
    -smp 1 \
    -m 128M \
    -kernel "$kernel" \
    -device loader,file="$fixture_bin",addr=0x41000000,force-raw=on \
    -semihosting-config enable=on,target=native \
    -display none \
    -audio none \
    -serial "file:$out" \
    -no-reboot
exit_code=$?

rm -f "$fixture_elf" "$fixture_bin"

if [ "$exit_code" -eq 0 ] && grep -Fqx "gabix arm userspace wrote" "$out"; then
    echo "[arm-virt-user] PASS"
    rm "$out"
    exit 0
fi

echo "[arm-virt-user] FAIL; serial log left at $out" >&2
exit 1
