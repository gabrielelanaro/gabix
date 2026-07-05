#!/bin/sh
# Create or reset the test disk image used by mini-os for the virtio-blk path.
#
# Lesson 10: format as ext2 so the kernel can parse a real filesystem.
# Lesson 14: write hello.txt + secret.txt into the ext2 filesystem via debugfs.
# Lesson 15: cross-compile a tiny x86-64 ELF and add it as /hello.elf.
# Lesson 17 (test framework): compile every tests/fixtures/*.rs into a user
#            ELF and add it to the disk. Each fixture is the user program
#            behind one E2E test in tests/.
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

# Lesson 14: write hello.txt and secret.txt into the ext2 filesystem.
HELLO_SRC=$(mktemp /tmp/mini-os-hello.XXXXXX.txt)
SECRET_SRC=$(mktemp /tmp/mini-os-secret.XXXXXX.txt)
echo "hello from disk" > "$HELLO_SRC"
echo "top secret" > "$SECRET_SRC"
printf 'write %s /hello.txt\nwrite %s /secret.txt\nquit\n' "$HELLO_SRC" "$SECRET_SRC" \
    | "$DEBUGFS" -w "$DISK_IMG" 2>&1 | grep -E '(inode|error)' || true
rm -f "$HELLO_SRC" "$SECRET_SRC"

# Lesson 15+: cross-compile every tests/fixtures/*.rs into a user ELF and
# write it to the disk. Two rustc flags matter:
#   --target x86_64-unknown-none   → bare-metal ELF, no host OS ABI
#   -C relocation-model=static     → ET_EXEC (fixed addresses), not PIE
# Without -C relocation-model=static, rustc emits ET_DYN, which would force
# the loader to handle dynamic relocations. We skip that.
if ! rustup target list --installed 2>/dev/null | grep -q '^x86_64-unknown-none$'; then
    echo "x86_64-unknown-none Rust target not installed." >&2
    echo "Run: rustup target add x86_64-unknown-none" >&2
    exit 1
fi

# Each line: <source-relative-to-repo-root> <dest-path-on-disk>
FIXTURES="tests/fixtures/hello.rs:/hello.elf
tests/fixtures/fork_smoke.rs:/fork_smoke.elf
tests/fixtures/inherit_wait.rs:/inherit_wait.elf"

for entry in $FIXTURES; do
    src=${entry%%:*}
    dest=${entry##*:}
    src_path="$(cd "$(dirname "$0")/.." && pwd)/$src"

    if [ ! -f "$src_path" ]; then
        echo "[make-disk] missing fixture: $src_path" >&2
        exit 1
    fi

    elf=$(mktemp /tmp/mini-os-fixture-XXXXXX.elf)
    rustc --target x86_64-unknown-none --edition 2021 \
          -C relocation-model=static \
          -o "$elf" "$src_path"

    file_info=$(file "$elf")
    case "$file_info" in
        *"ELF 64-bit LSB executable, x86-64"*)
            echo "[make-disk] $dest ← $src: OK"
            ;;
        *)
            echo "[make-disk] $dest: unexpected file type: $file_info" >&2
            rm -f "$elf"
            exit 1
            ;;
    esac

    printf 'write %s %s\nquit\n' "$elf" "$dest" \
        | "$DEBUGFS" -w "$DISK_IMG" 2>&1 | grep -E '(inode|error)' || true
    rm -f "$elf"
done

# Verify
echo "Created $DISK_IMG ($SIZE_MB MB, ext2)"
echo "ext2 magic at offset 1080 (should be 53 EF):"
dd if="$DISK_IMG" bs=1 skip=1080 count=2 2>/dev/null | hexdump -C
echo "Root directory listing:"
"$DEBUGFS" -R "ls /" "$DISK_IMG" 2>&1 | head -3
