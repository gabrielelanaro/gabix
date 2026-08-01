#!/bin/sh

set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
output=${1:-"$repo_root/target/gabix-pi2.img"}
firmware_revision=8881141ab778862ce3750d5854c43f0ebb567494
boot_start=2048
root_start=133120
temporary=$(mktemp -d /tmp/gabix-pi2-image.XXXXXX)

cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT INT TERM

mkdir -p "$(dirname "$output")"
kernel=$(sh "$script_dir/build-arm.sh" raspi2b-hardware | tail -1)
root="$temporary/root.ext2"
sh "$script_dir/make-arm-disk.sh" tests/fixtures/arm_syscall.rs "$root" >/dev/null

if [ -n "${PI_FIRMWARE_DIR:-}" ]; then
    firmware_dir=$PI_FIRMWARE_DIR
else
    firmware_dir="$repo_root/target/pi-firmware-$firmware_revision"
    mkdir -p "$firmware_dir"
    for name in bootcode.bin start.elf fixup.dat; do
        if [ ! -s "$firmware_dir/$name" ]; then
            curl -fL --retry 3 \
                "https://raw.githubusercontent.com/raspberrypi/firmware/$firmware_revision/boot/$name" \
                -o "$firmware_dir/$name"
        fi
    done
fi

for name in bootcode.bin start.elf fixup.dat; do
    if [ ! -s "$firmware_dir/$name" ]; then
        echo "missing Raspberry Pi firmware file: $firmware_dir/$name" >&2
        exit 1
    fi
done

boot="$temporary/boot.fat16"
python3 "$script_dir/make-fat16.py" "$boot" "$boot_start" \
    "BOOTCODE.BIN=$firmware_dir/bootcode.bin" \
    "START.ELF=$firmware_dir/start.elf" \
    "FIXUP.DAT=$firmware_dir/fixup.dat" \
    "KERNEL7.IMG=$kernel" \
    "CONFIG.TXT=$repo_root/targets/armv7/boot/config.txt"

python3 "$script_dir/assemble-mbr.py" "$output" \
    "0x0e:$boot_start:$boot" \
    "0x83:$root_start:$root"
python3 "$script_dir/verify-pi2-image.py" "$output" >/dev/null
printf '%s\n' "$output"
