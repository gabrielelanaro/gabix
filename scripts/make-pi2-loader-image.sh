#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 <official-raspberry-pi-image> [output-image]" >&2
    exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
source_image=$1
output_image=${2:-$repo_root/target/gabix-pi2-loader.img}
kernel=$(sh "$script_dir/build-arm.sh" pi2-loader | tail -1)

python3 "$script_dir/make-pi2-uart-smoke-from-official.py" \
    "$source_image" "$kernel" "$output_image" \
    --config "$repo_root/targets/armv7/boot/config.txt"

printf 'loader_image=%s\n' "$output_image"
