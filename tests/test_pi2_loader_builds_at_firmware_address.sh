#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
image=$(sh "$repo_root/scripts/build-arm.sh" pi2-loader | tail -1)
rust_sysroot=$(rustc --print sysroot)
rust_host=$(rustc -vV | sed -n 's/^host: //p')
readobj="$rust_sysroot/lib/rustlib/$rust_host/bin/llvm-readobj"
elf="$repo_root/targets/armv7/target/armv7a-none-eabihf/debug/pi2-loader"

if [ "$(basename "$image")" = "kernel-pi2-loader.img" ] &&
    [ -s "$image" ] &&
    "$readobj" --file-header "$elf" | grep -Fq "Entry: 0x8000"; then
    echo "[pi2-loader-build] PASS"
    exit 0
fi

echo "[pi2-loader-build] FAIL" >&2
exit 1
