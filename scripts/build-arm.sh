#!/bin/sh

set -eu

if [ "$#" -ne 1 ] || { [ "$1" != "virt" ] && [ "$1" != "raspi2b" ]; }; then
    echo "usage: $0 <virt|raspi2b>" >&2
    exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
platform=$1
manifest="$repo_root/targets/armv7/Cargo.toml"
linker_script="$repo_root/targets/armv7/linker.ld"
if [ "$platform" = "virt" ]; then
    kernel_base=0x40010000
else
    kernel_base=0x00010000
fi
rust_sysroot=$(rustc --print sysroot)
llvm_objcopy="$rust_sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objcopy"
elf="$repo_root/targets/armv7/target/armv7a-none-eabihf/debug/gabix-armv7"
image="$repo_root/targets/armv7/target/armv7a-none-eabihf/debug/kernel-$platform.img"

if [ ! -x "$llvm_objcopy" ]; then
    echo "llvm-objcopy was not found in the active Rust toolchain" >&2
    exit 1
fi

RUSTFLAGS="-C relocation-model=static -C link-arg=-T$linker_script -C link-arg=--defsym=KERNEL_BASE=$kernel_base" \
    cargo build \
        --manifest-path "$manifest" \
        --target armv7a-none-eabihf \
        --no-default-features \
        --features "$platform"

"$llvm_objcopy" -O binary "$elf" "$image"
printf '%s\n' "$image"
