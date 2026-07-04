#!/bin/sh
# Create or reset the test disk image used by mini-os for the virtio-blk path.
#
# Lesson 10: format as ext2 so the kernel can parse a real filesystem.
# Lesson 14: write hello.txt + secret.txt into the ext2 filesystem via debugfs.
# Lesson 15: cross-compile a tiny x86-64 ELF and add it as /hello.elf, so the
#            kernel has a real binary to parse with the ELF header parser.
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
# debugfs's `write` command copies a host file into the ext2 filesystem.
# We pipe commands via stdin and end with `quit` to exit cleanly.
HELLO_SRC=$(mktemp /tmp/mini-os-hello.XXXXXX.txt)
SECRET_SRC=$(mktemp /tmp/mini-os-secret.XXXXXX.txt)
echo "hello from disk" > "$HELLO_SRC"
echo "top secret" > "$SECRET_SRC"
printf 'write %s /hello.txt\nwrite %s /secret.txt\nquit\n' "$HELLO_SRC" "$SECRET_SRC" \
    | "$DEBUGFS" -w "$DISK_IMG" 2>&1 | grep -E '(inode|error)' || true
rm -f "$HELLO_SRC" "$SECRET_SRC"

# Lesson 15: cross-compile a tiny x86-64 ELF and add it as /hello.elf.
#
# Two rustc flags matter:
#   --target x86_64-unknown-none   → bare-metal ELF, no host OS ABI
#   -C relocation-model=static     → ET_EXEC (fixed addresses), not PIE
# Without -C relocation-model=static, rustc emits ET_DYN, which would force
# the loader to handle dynamic relocations in Lesson 16. We skip that.
if ! rustup target list --installed 2>/dev/null | grep -q '^x86_64-unknown-none$'; then
    echo "x86_64-unknown-none Rust target not installed." >&2
    echo "Run: rustup target add x86_64-unknown-none" >&2
    exit 1
fi

HELLO_ELF_SRC=$(mktemp /tmp/mini-os-hello-user-XXXXXX.rs)
cat > "$HELLO_ELF_SRC" <<'EOF'
#![no_std]
#![no_main]
#[no_mangle]
pub extern "C" fn _start() -> ! { loop {} }
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
EOF

HELLO_ELF=$(mktemp /tmp/mini-os-hello-user-XXXXXX.elf)
rustc --target x86_64-unknown-none --edition 2021 \
      -C relocation-model=static \
      -o "$HELLO_ELF" "$HELLO_ELF_SRC"

# Sanity-check: confirm it's an ET_EXEC x86-64 ELF.
FILE_INFO=$(file "$HELLO_ELF")
case "$FILE_INFO" in
    *"ELF 64-bit LSB executable, x86-64"*)
        echo "[make-disk] hello.elf: $FILE_INFO"
        ;;
    *)
        echo "[make-disk] unexpected file type: $FILE_INFO" >&2
        rm -f "$HELLO_ELF_SRC" "$HELLO_ELF"
        exit 1
        ;;
esac

# Write it into ext2 via debugfs, same pattern as hello.txt.
printf 'write %s /hello.elf\nquit\n' "$HELLO_ELF" \
    | "$DEBUGFS" -w "$DISK_IMG" 2>&1 | grep -E '(inode|error)' || true
rm -f "$HELLO_ELF_SRC" "$HELLO_ELF"

# Verify
echo "Created $DISK_IMG ($SIZE_MB MB, ext2)"
echo "ext2 magic at offset 1080 (should be 53 EF):"
dd if="$DISK_IMG" bs=1 skip=1080 count=2 2>/dev/null | hexdump -C
echo "Root directory listing:"
"$DEBUGFS" -R "ls /" "$DISK_IMG" 2>&1 | head -3
