#!/bin/sh

set -eu

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <arm-fixture-source> <output-image>" >&2
    exit 2
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
fixture=$1
disk=$2

sh "$script_dir/make-disk.sh" tests/fixtures/hello.rs "$disk" >/dev/null

debugfs=$(command -v debugfs || command -v /usr/sbin/debugfs ||
    { command -v brew >/dev/null 2>&1 && brew --prefix e2fsprogs 2>/dev/null | xargs -I{} printf '%s/sbin/debugfs' '{}'; } || true)
if [ -z "$debugfs" ]; then
    echo "debugfs not found" >&2
    exit 1
fi

fixture_elf=$(mktemp /tmp/mini-os-arm-init.XXXXXX)
rustc \
    --target armv7a-none-eabihf \
    --edition 2021 \
    -C panic=abort \
    -C relocation-model=static \
    -C link-arg=-T"$repo_root/tests/fixtures/armv7-user.ld" \
    -C link-arg=-z \
    -C link-arg=max-page-size=0x1000 \
    -o "$fixture_elf" \
    "$repo_root/$fixture"

printf 'rm /init.elf\nwrite %s /init.elf\nquit\n' "$fixture_elf" \
    | "$debugfs" -w "$disk" >/dev/null 2>&1
rm -f "$fixture_elf"

printf '%s\n' "$disk"
