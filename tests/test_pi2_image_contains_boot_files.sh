#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST Pi 2 image contains boot files ==="
temporary=$(mktemp -d /tmp/mini-os-pi2-image.XXXXXX)
firmware="$temporary/firmware"
image="$temporary/gabix-pi2.img"
mkdir "$firmware"
printf 'test bootcode\n' >"$firmware/bootcode.bin"
printf 'test start firmware\n' >"$firmware/start.elf"
printf 'test fixup\n' >"$firmware/fixup.dat"

PI_FIRMWARE_DIR="$firmware" sh "$REPO_ROOT/scripts/make-pi2-sd-image.sh" "$image" >/dev/null

if python3 "$REPO_ROOT/scripts/verify-pi2-image.py" "$image" >/dev/null; then
    echo "[pi2-sd-image] PASS"
    rm -rf "$temporary"
    exit 0
fi

echo "[pi2-sd-image] FAIL; image left at $image" >&2
exit 1
