#!/bin/sh
# Create or reset the test disk image used by mini-os for the virtio-blk path.
#
# Lesson 10: format as ext2 so the kernel can parse a real filesystem.
# Lesson 14: write a hello.txt file into the ext2 filesystem via debugfs,
#            so resolve_path("/hello.txt") has something to find.
#
# Output: disk.img (in the project root)
#
# Usage:
#   ./scripts/make-disk.sh

set -e

DISK_IMG=disk.img
SIZE_MB=1

# Find mkfs.ext2 and debugfs — keg-only on macOS (brew install e2fsprogs).
E2SPROGS_SBIN=$(brew --prefix e2fsprogs 2>/dev/null)/sbin
MKFS_EXT2=$(command -v mkfs.ext2 || \
            command -v mke2fs || \
            command -v "$E2SPROGS_SBIN/mkfs.ext2" || \
            command -v /usr/sbin/mkfs.ext2 || \
            true)
DEBUGFS=$(command -v debugfs || \
          command -v "$E2SPROGS_SBIN/debugfs" || \
          command -v /usr/sbin/debugfs || \
          true)

if [ -z "$MKFS_EXT2" ]; then
    echo "mkfs.ext2 not found." >&2
    echo "On macOS: brew install e2fsprogs" >&2
    exit 1
fi
if [ -z "$DEBUGFS" ]; then
    echo "debugfs not found." >&2
    echo "On macOS: brew install e2fsprogs" >&2
    exit 1
fi

# Create the file (sparse, all zeros), then format as ext2.
truncate -s "${SIZE_MB}M" "$DISK_IMG"
"$MKFS_EXT2" -F "$DISK_IMG" 2>&1 | grep -E '(ext2|blocks|inodes)' | head -5

# Lesson 14: write hello.txt into the ext2 filesystem.
# debugfs's `write` command copies a host file into the ext2 filesystem.
# We pipe commands via stdin and end with `quit` to exit cleanly
# (debugfs stays interactive after -R runs a single command).
HELLO_SRC=$(mktemp /tmp/mini-os-hello.XXXXXX.txt)
echo "hello from disk" > "$HELLO_SRC"
printf 'write %s /hello.txt\nquit\n' "$HELLO_SRC" | "$DEBUGFS" -w "$DISK_IMG" 2>&1 | grep -E '(inode|error)' || true
rm -f "$HELLO_SRC"

# Verify
echo "Created $DISK_IMG ($SIZE_MB MB, ext2)"
echo "ext2 magic at offset 1080 (should be 53 EF):"
dd if="$DISK_IMG" bs=1 skip=1080 count=2 2>/dev/null | hexdump -C
echo "Root directory listing:"
"$DEBUGFS" -R "ls /" "$DISK_IMG" 2>&1 | head -3
