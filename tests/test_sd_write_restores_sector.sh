#!/bin/sh
# Physical Pi 2B test; start with the UART loader waiting for an upload.
set -eu
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 <uart-device> [recovery-image-gap|managed-layout-scratch]" >&2
    exit 2
fi
layout=${2:-recovery-image-gap}
case "$layout" in
    recovery-image-gap|managed-layout-scratch) ;;
    *) echo "Unknown scratch layout: $layout" >&2; exit 2 ;;
esac
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
run_dir="$repo_root/target/runs/$(date +%Y%m%d-%H%M%S)-sd-write"
mkdir -p "$run_dir"
kernel=$(GABIX_ARM_PROFILE=release GABIX_SD_CHECK_WRITE="$layout" \
    sh "$repo_root/scripts/build-arm.sh" pi2-sd-check | tail -1)
python3 "$repo_root/scripts/upload-pi2-kernel.py" "$1" "$kernel" \
    --inter-byte-delay-us 500 --retries 1 --wait-for-reboot --reboot-timeout 25 \
    >"$run_dir/uart.log" 2>&1
if ! grep -Fqx 'SD check: PASS write/read/restore/neighbors' "$run_dir/uart.log"; then
    echo "FAIL: SD write/read-back; see $run_dir/uart.log" >&2
    exit 1
fi
echo "PASS: SD pattern read back, original restored, neighbors unchanged, UART loader ready"
