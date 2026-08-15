#!/bin/sh

set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 <user-source.rs> [output.elf]" >&2
    exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
source_path=$1
source_name=$(basename "$source_path" .rs)
output=${2:-$repo_root/target/$source_name.elf}

case "$source_path" in
    /*) ;;
    *) source_path=$repo_root/$source_path ;;
esac

mkdir -p "$(dirname "$output")"
rustc \
    --target armv7a-none-eabihf \
    --edition 2021 \
    -C panic=abort \
    -C relocation-model=static \
    -C opt-level=s \
    -C strip=symbols \
    -C link-arg=-T"$repo_root/tests/fixtures/armv7-user.ld" \
    -C link-arg=-z \
    -C link-arg=max-page-size=0x1000 \
    -o "$output" \
    "$source_path"

printf '%s\n' "$output"
