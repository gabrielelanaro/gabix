#!/bin/sh

. "$(dirname "$0")/common.sh"

echo "=== TEST MBR finds Linux partition ==="
if cargo +stable test --quiet --manifest-path "$REPO_ROOT/crates/kernel-core/Cargo.toml" \
    --target x86_64-unknown-linux-gnu --test mbr_partitions; then
    echo "[mbr-partition] PASS"
    exit 0
fi

echo "[mbr-partition] FAIL" >&2
exit 1
