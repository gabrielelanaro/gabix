#!/bin/sh
# Build a standalone no_std program with the returning kernel probe ABI.
set -eu
if [ "$#" -ne 2 ]; then
    echo "usage: $0 probe.rs output.img" >&2
    exit 2
fi
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -p "$(dirname -- "$2")"
tools="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin"
rustc --edition 2021 --target armv7a-none-eabihf -C opt-level=s -C panic=abort \
    -C relocation-model=static -C link-arg="-T$root/targets/armv7/probes/probe.ld" \
    "$1" -o "$2.elf"
"$tools/llvm-objcopy" -O binary "$2.elf" "$2"
