#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
image=$(sh "$repo_root/scripts/build-arm.sh" pi2-dev | tail -1)
elf="$repo_root/targets/armv7/target/armv7a-none-eabihf/debug/gabix-armv7"
rust_sysroot=$(rustc --print sysroot)
rust_host=$(rustc -vV | sed -n 's/^host: //p')
readobj="$rust_sysroot/lib/rustlib/$rust_host/bin/llvm-readobj"

[ "$(basename "$image")" = kernel-pi2-dev.img ]
[ -s "$image" ]
"$readobj" --file-headers "$elf" | grep -Eq 'Entry: 0x0*100000$'
echo "[pi2-dev-build] PASS"
