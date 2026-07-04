#!/bin/sh
# Create or reset the test disk image used by mini-os for the virtio-blk path.
#
# Lesson 10: format as ext2 so the kernel can parse a real filesystem.
# Earlier lessons wrote a "MINIOS_DISK_v0\n" signature at sector 0 — that
# got overwritten by mkfs.ext2. Lesson 10 verifies the ext2 magic instead
# (0xEF53 at byte offset 1080).
#
# Output: disk.img (in the project root)
#
# Usage:
#   ./scripts/make-disk.sh

set -e

DISK_IMG=disk.img
SIZE_MB=1

# Find mkfs.ext2 — it's keg-only on macOS (brew install e2fsprogs).
MKFS_EXT2=$(command -v mkfs.ext2 || \
            command -v mke2fs || \
            command -v /opt/homebrew/opt/e2fsprogs/sbin/mkfs.ext2 || \
            command -v /usr/sbin/mkfs.ext2 || \
            true)
if [ -z "$MKFS_EXT2" ]; then
    echo "mkfs.ext2 not found." >&2
    echo "On macOS: brew install e2fsprogs" >&2
    exit 1
fi

# Create the file (sparse, all zeros), then format as ext2.
truncate -s "${SIZE_MB}M" "$DISK_IMG"
"$MKFS_EXT2" -F "$DISK_IMG" 2>&1 | grep -E '(ext2|blocks|inodes)' | head -5

# Verify: bytes at offset 1080 should be 53 EF (ext2 magic 0xEF53 in LE)
echo "Created $DISK_IMG ($SIZE_MB MB, ext2)"
echo "ext2 magic at offset 1080 (should be 53 EF):"
dd if="$DISK_IMG" bs=1 skip=1080 count=2 2>/dev/null | hexdump -C

