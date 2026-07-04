//! ext2 filesystem parser — Lesson 10: superblock.
//!
//! ext2 is a fixed-layout on-disk format. The superblock lives at byte
//! offset 1024 (sector 2) and tells the kernel everything it needs to
//! navigate the filesystem: block size, total inode/block counts,
//! where the first data lives, etc.
//!
//! Field offsets and sizes come from `struct ext2_super_block` in
//! Linux's include/uapi/linux/ext2_fs.h. We parse only the fields
//! we need (the first ~60 bytes); the rest of the 1024-byte struct
//! is metadata for journaling, quotas, etc. that we don't touch.

use crate::println;

/// ext2 magic number — appears at byte offset 56 of the superblock
/// (byte offset 1080 of the disk, = sector 2 byte 56). If this isn't
/// 0xEF53, the disk either isn't ext2 or we're reading the wrong offset.
pub const EXT2_MAGIC: u16 = 0xEF53;

/// ext2 root inode number. By convention inode 2 is the root directory
/// (inode 1 is reserved for "bad blocks" file). We'll look this up in
/// Lesson 12.
pub const EXT2_ROOT_INO: u32 = 2;

/// Default block size when s_log_block_size == 0. Other valid values:
/// 1 -> 2048, 2 -> 4096.
const EXT2_MIN_BLOCK_SIZE: u32 = 1024;

/// The parsed superblock. We don't model the full 1024-byte on-disk
/// struct — just the fields a teaching kernel needs to read inodes and
/// walk directories.
///
/// Construction: `Superblock::parse(bytes)` where `bytes` is the full
/// 1024-byte superblock read from disk (sectors 2 and 3 concatenated).
#[derive(Debug)]
pub struct Superblock {
    /// Total inodes in the filesystem. Fixed at mkfs time.
    pub inodes_count: u32,
    /// Total filesystem blocks. Unit: ext2 blocks (default 1024 B).
    pub blocks_count: u32,
    /// Blocks reserved for superuser (root). Excluded from free count.
    pub r_blocks_count: u32,
    /// Currently unused blocks. Decrements on writes, increments on deletes.
    pub free_blocks_count: u32,
    /// Currently unused inodes. Same idea.
    pub free_inodes_count: u32,
    /// First block index that holds filesystem data. For 1024 B block
    /// filesystems this is 1 (block 0 is the boot sector). For larger
    /// block sizes it's 0 because the boot sector fits in block 0.
    pub first_data_block: u32,
    /// Computed block size in bytes: 1024 << s_log_block_size.
    pub block_size: u32,
    /// The magic number — should equal `EXT2_MAGIC` (0xEF53).
    pub magic: u16,
    /// Filesystem state. 1 = clean, 2 = has errors. mkfs leaves it 1.
    pub state: u32,
}

impl Superblock {
    /// Parse a 1024-byte slice into a Superblock. Each field is read at
    /// its spec-mandated offset via `from_le_bytes` — ext2 is
    /// little-endian, x86 is little-endian, but using from_le_bytes
    /// explicitly documents the assumption and handles alignment
    /// without needing repr(C) transmute tricks.
    pub fn parse(bytes: &[u8]) -> Self {
        assert!(
            bytes.len() >= 1024,
            "superblock needs 1024 bytes, got {}",
            bytes.len()
        );

        // Helper: read a little-endian u32 at byte offset.
        let u32_at = |off: usize| -> u32 {
            u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
        };
        // Helper: read a little-endian u16 at byte offset.
        let u16_at = |off: usize| -> u16 {
            u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap())
        };

        let s_log_block_size = u32_at(24);
        let block_size = EXT2_MIN_BLOCK_SIZE << s_log_block_size;

        Self {
            inodes_count:       u32_at(0),
            blocks_count:       u32_at(4),
            r_blocks_count:     u32_at(8),
            free_blocks_count:  u32_at(12),
            free_inodes_count:  u32_at(16),
            first_data_block:   u32_at(20),
            block_size,
            magic:              u16_at(56),
            state:              u32_at(58),
        }
    }

    /// Validate the magic number. Returns true if the disk is ext2.
    pub fn is_ext2(&self) -> bool {
        self.magic == EXT2_MAGIC
    }

    /// Pretty-print the parsed fields for boot-time verification.
    pub fn print(&self) {
        println!("[ext2] parsed superblock:");
        println!(
            "[ext2]   magic         = 0x{:04X}  {}",
            self.magic,
            if self.is_ext2() { "✓ ext2" } else { "✗ NOT ext2" }
        );
        println!("[ext2]   inodes_count  = {}", self.inodes_count);
        println!("[ext2]   blocks_count  = {}", self.blocks_count);
        println!("[ext2]   free_inodes   = {}", self.free_inodes_count);
        println!("[ext2]   free_blocks   = {}", self.free_blocks_count);
        println!("[ext2]   first_data    = {}", self.first_data_block);
        println!("[ext2]   block_size    = {} bytes", self.block_size);
        println!("[ext2]   state         = {} ({})",
            self.state,
            if self.state == 1 { "clean" } else { "needs check" });
    }
}
