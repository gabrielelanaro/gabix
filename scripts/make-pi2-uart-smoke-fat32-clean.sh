#!/bin/sh

set -eu

if [ "$#" -gt 1 ]; then
    echo "usage: $0 [output-image]" >&2
    exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
output=${1:-"$repo_root/target/pi2-uart-smoke-fat32-clean.img"}
firmware_dir=${PI_FIRMWARE_DIR:-"$repo_root/target/pi-firmware-8aca5762f984f6decbeda294cee8418966c3d8d3"}
partition_start=16384
partition_sectors=1048576
temporary=$(mktemp -d /tmp/gabix-pi2-uart-fat32-clean.XXXXXX)
boot="$temporary/boot.img"
mountpoint="$temporary/mount"
device=""
mounted_device=""

cleanup() {
    if [ -n "$mounted_device" ]; then
        hdiutil detach "$mounted_device" >/dev/null 2>&1 || true
    fi
    if [ -n "$device" ]; then
        hdiutil detach "$device" >/dev/null 2>&1 || true
    fi
    rm -rf "$temporary"
}
trap cleanup EXIT INT TERM

for name in bootcode.bin start.elf fixup.dat; do
    if [ ! -s "$firmware_dir/$name" ]; then
        echo "missing firmware file: $firmware_dir/$name" >&2
        exit 1
    fi
done

mkdir -p "$(dirname "$output")" "$mountpoint"
truncate -s 512m "$boot"
device=$(hdiutil attach -nomount "$boot" | awk '/^\/dev\/disk/ { print $1; exit }')
raw_device=$(printf '%s\n' "$device" | sed 's#^/dev/disk#/dev/rdisk#')
newfs_msdos -F 32 -v bootfs "$raw_device" >/dev/null
hdiutil detach "$device" >/dev/null
device=""

mounted_device=$(hdiutil attach -nobrowse -mountpoint "$mountpoint" "$boot" | awk '/^\/dev\/disk/ { print $1; exit }')
cp "$firmware_dir/bootcode.bin" "$mountpoint/BOOTCODE.BIN"
cp "$firmware_dir/start.elf" "$mountpoint/START.ELF"
cp "$firmware_dir/fixup.dat" "$mountpoint/FIXUP.DAT"
cp "$repo_root/target/pi2-uart-smoke/kernel7.img" "$mountpoint/KERNEL7.IMG"
cp "$repo_root/targets/armv7/boot/config.txt" "$mountpoint/CONFIG.TXT"
sync
hdiutil detach "$mounted_device" >/dev/null
mounted_device=""

python3 "$script_dir/assemble-mbr.py" "$output" "0x0c:$partition_start:$boot"
python3 - "$output" <<'PY'
from pathlib import Path
import sys

image = Path(sys.argv[1])
with image.open("r+b") as stream:
    stream.seek(446)
    stream.write(b"\x00")
PY
printf '%s\n' "$output"
