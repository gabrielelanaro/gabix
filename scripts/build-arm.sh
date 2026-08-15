#!/bin/sh

set -eu

if [ "$#" -ne 1 ] ||
    { [ "$1" != "virt" ] && [ "$1" != "raspi2b" ] &&
        [ "$1" != "raspi2b-hardware" ] && [ "$1" != "raspi2b-lifecycle" ] &&
        [ "$1" != "raspi2b-uart-user" ] && [ "$1" != "pi2-loader" ] &&
        [ "$1" != "pi2-dev" ]; }; then
    echo "usage: $0 <virt|raspi2b|raspi2b-hardware|raspi2b-lifecycle|raspi2b-uart-user|pi2-loader|pi2-dev>" >&2
    exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
platform=$1
manifest="$repo_root/targets/armv7/Cargo.toml"
linker_script="$repo_root/targets/armv7/linker.ld"
binary=gabix-armv7
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
    raspi2b-lifecycle)
        kernel_base=0x00010000
        feature=pi-lifecycle
        image_name=kernel-raspi2b-lifecycle.img
        ;;
    raspi2b-uart-user)
        kernel_base=0x00010000
        feature=pi-uart-user
        image_name=kernel-raspi2b-uart-user.img
        ;;
    pi2-loader)
        kernel_base=0x00008000
        feature=
        binary=pi2-loader
        image_name=kernel-pi2-loader.img
        ;;
    pi2-dev)
        kernel_base=0x00100000
        feature=pi-hardware,pi-uart-user
        image_name=kernel-pi2-dev.img
        ;;
esac
rust_sysroot=$(rustc --print sysroot)
rust_host=$(rustc -vV | sed -n 's/^host: //p')
llvm_objcopy="$rust_sysroot/lib/rustlib/$rust_host/bin/llvm-objcopy"
elf="$repo_root/targets/armv7/target/armv7a-none-eabihf/debug/$binary"
image="$repo_root/targets/armv7/target/armv7a-none-eabihf/debug/$image_name"

if [ ! -x "$llvm_objcopy" ]; then
    echo "llvm-objcopy was not found in the active Rust toolchain" >&2
    exit 1
fi

set -- cargo build \
    --manifest-path "$manifest" \
    --target armv7a-none-eabihf \
    --bin "$binary" \
    --no-default-features
if [ -n "$feature" ]; then
    set -- "$@" --features "$feature"
fi
RUSTFLAGS="-C relocation-model=static -C link-arg=-T$linker_script -C link-arg=--defsym=KERNEL_BASE=$kernel_base" \
    "$@"

"$llvm_objcopy" -O binary "$elf" "$image"
printf '%s\n' "$image"
