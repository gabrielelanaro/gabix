#!/bin/sh
# Create or reset the test disk image used by mini-os for the virtio-blk path.
#
# The disk is a 1 MB raw image with a known signature at sector 0 so Lesson 8's
# first read can verify the bytes match what we wrote here.
#
# Output: disk.img (in the project root)
#
# Usage:
#   ./scripts/make-disk.sh

set -e

DISK_IMG=disk.img
SIZE_MB=1
SIGNATURE='MINIOS_DISK_v0'

# Create the file (sparse, all zeros)
truncate -s "${SIZE_MB}M" "$DISK_IMG"

# Write the signature at offset 0 followed by a newline
printf '%s\n' "$SIGNATURE" | dd of="$DISK_IMG" conv=notrunc bs=1 count=$(( ${#SIGNATURE} + 1 )) 2>/dev/null

# Verify
echo "Created $DISK_IMG ($SIZE_MB MB)"
echo "First 16 bytes (should be '$SIGNATURE\\n' then zeros):"
hexdump -C "$DISK_IMG" | head -1
