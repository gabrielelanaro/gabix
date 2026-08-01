#!/bin/sh

set -eu

if [ "$#" -ne 1 ] ||
    { [ "$1" != "virt" ] && [ "$1" != "raspi2b" ] && [ "$1" != "raspi2b-hardware" ]; }; then
    echo "usage: $0 <virt|raspi2b|raspi2b-hardware>" >&2
    exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
platform=$1
manifest="$repo_root/targets/armv7/Cargo.toml"
linker_script="$repo_root/targets/armv7/linker.ld"
case "$platform" in
    virt)
        kernel_base=0x40010000
        feature=virt
        image_name=kernel-virt.img
        ;;
    raspi2b)
        kernel_base=0x00010000
        feature=raspi2b
        image_name=kernel-raspi2b.img
        ;;
    raspi2b-hardware)
        kernel_base=0x00008000
        feature=pi-hardware
        image_name=kernel7.img
        ;;
esac
rust_sysroot=$(rustc --print sysroot)
llvm_objcopy="$rust_sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-objcopy"
elf="$repo_root/targets/armv7/target/armv7a-none-eabihf/debug/gabix-armv7"
image="$repo_root/targets/armv7/target/armv7a-none-eabihf/debug/$image_name"

if [ ! -x "$llvm_objcopy" ]; then
    echo "llvm-objcopy was not found in the active Rust toolchain" >&2
    exit 1
fi

RUSTFLAGS="-C relocation-model=static -C link-arg=-T$linker_script -C link-arg=--defsym=KERNEL_BASE=$kernel_base" \
    cargo build \
        --manifest-path "$manifest" \
        --target armv7a-none-eabihf \
        --no-default-features \
        --features "$feature"

"$llvm_objcopy" -O binary "$elf" "$image"
printf '%s\n' "$image"
