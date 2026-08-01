#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST Raspberry Pi 2B builds hardware kernel ==="
image=$(sh "$REPO_ROOT/scripts/build-arm.sh" raspi2b-hardware | tail -1)
toolchain=$(rustc --print sysroot)
readobj="$toolchain/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-readobj"
elf="$REPO_ROOT/targets/armv7/target/armv7a-none-eabihf/debug/gabix-armv7"

if [ "$(basename "$image")" = "kernel7.img" ] &&
    [ -s "$image" ] &&
    "$readobj" --file-header "$elf" | grep -Fq "Entry: 0x8000"; then
    echo "[raspi2b-hardware-kernel] PASS"
    exit 0
fi

echo "[raspi2b-hardware-kernel] FAIL" >&2
exit 1
