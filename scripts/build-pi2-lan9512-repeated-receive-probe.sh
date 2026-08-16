#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
output=${1:-"$repo_root/target/pi2-lan9512-repeated-receive-probe.img"}
mkdir -p "$(dirname -- "$output")"

rust_sysroot=$(rustc --print sysroot)
rust_host=$(rustc -vV | sed -n 's/^host: //p')
llvm_objcopy="$rust_sysroot/lib/rustlib/$rust_host/bin/llvm-objcopy"
elf="$repo_root/targets/armv7/target/armv7a-none-eabihf/release/pi2-lan9512-receive-probe"
RUSTFLAGS="-C relocation-model=static -C link-arg=-T$repo_root/targets/armv7/linker.ld -C link-arg=--defsym=KERNEL_BASE=0x00100000" \
    cargo build \
        --manifest-path "$repo_root/targets/armv7/Cargo.toml" \
        --target armv7a-none-eabihf \
        --bin pi2-lan9512-receive-probe \
        --release \
        --no-default-features \
        --features pi-hardware,pi-network-repeated-receive-probe

"$llvm_objcopy" -O binary "$elf" "$output"
printf '%s\n' "$output"
