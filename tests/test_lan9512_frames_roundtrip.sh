#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST LAN9512 frames roundtrip ==="
if cargo +stable test --quiet --manifest-path "$REPO_ROOT/crates/kernel-core/Cargo.toml" \
    --target x86_64-unknown-linux-gnu --test smsc95xx_frames; then
    echo "[lan9512-frames] PASS"
    exit 0
fi

echo "[lan9512-frames] FAIL" >&2
exit 1
