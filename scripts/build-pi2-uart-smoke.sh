#!/bin/sh

set -eu

if [ "$#" -gt 1 ]; then
    echo "usage: $0 [output-kernel]" >&2
    exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
output=${1:-"$repo_root/target/pi2-uart-smoke/kernel7.img"}
output_dir=$(dirname "$output")
mkdir -p "$output_dir"
kernel_base=${GABIX_PI2_UART_SMOKE_KERNEL_BASE:-0x8000}

rust_sysroot=$(rustc --print sysroot)
rust_host=$(rustc -vV | sed -n 's/^host: //p')
llvm_objcopy="$rust_sysroot/lib/rustlib/$rust_host/bin/llvm-objcopy"
elf="$output_dir/uart-smoke.elf"

if [ ! -x "$llvm_objcopy" ]; then
    echo "llvm-objcopy was not found in the active Rust toolchain" >&2
    exit 1
fi

rustc \
    --target armv7a-none-eabihf \
    --edition 2021 \
    -C panic=abort \
    -C relocation-model=static \
    -C link-arg=-T"$repo_root/targets/armv7/linker.ld" \
    -C link-arg=--defsym=KERNEL_BASE="$kernel_base" \
    -o "$elf" \
    "$repo_root/targets/armv7/src/uart_smoke.rs"

"$llvm_objcopy" -O binary "$elf" "$output"
rm -f "$elf"
printf '%s\n' "$output"
