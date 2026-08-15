#!/bin/sh

set -eu

if [ "$#" -gt 1 ]; then
    echo "usage: $0 [output-image]" >&2
    exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
output=${1:-"$repo_root/target/pi2-uart-smoke.img"}
firmware_revision=8881141ab778862ce3750d5854c43f0ebb567494
boot_start=2048
temporary=$(mktemp -d /tmp/gabix-pi2-uart-smoke.XXXXXX)

cleanup() {
    rm -rf "$temporary"
}
trap cleanup EXIT INT TERM

mkdir -p "$(dirname "$output")"
kernel=$(sh "$script_dir/build-pi2-uart-smoke.sh" | tail -1)

if [ -n "${PI_FIRMWARE_DIR:-}" ]; then
    firmware_dir=$PI_FIRMWARE_DIR
else
    firmware_dir="$repo_root/target/pi-firmware-$firmware_revision"
fi

for name in bootcode.bin start.elf fixup.dat; do
    if [ ! -s "$firmware_dir/$name" ]; then
        echo "missing Raspberry Pi firmware file: $firmware_dir/$name" >&2
        echo "Build the normal image once or set PI_FIRMWARE_DIR." >&2
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
    "0x0e:$boot_start:$boot"
printf '%s\n' "$output"
