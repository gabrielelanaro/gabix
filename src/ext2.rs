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
use crate::virtio::VirtioBlk;
use alloc::vec::Vec;

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

/// Read the ext2 superblock from disk. The superblock is 1024 bytes
/// starting at byte offset 1024 (sector 2). Since `read_sector` returns
/// 512 bytes at a time, we read two sectors (2 and 3) and concatenate.
///
/// Future optimization: a `read_bytes(offset, len)` primitive that
/// handles multi-sector reads internally. For Lesson 10, two calls
/// plus a concat is fine.
pub fn read_superblock(dev: &mut VirtioBlk) -> Superblock {
    println!("[ext2] reading superblock (sectors 2 and 3)");

    let sector2 = dev.read_sector(2);
    let sector3 = dev.read_sector(3);

    let mut bytes = [0u8; 1024];
    bytes[..512].copy_from_slice(&sector2);
    bytes[512..].copy_from_slice(&sector3);

    Superblock::parse(&bytes)
}

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
    pub state: u16,
    /// How many inodes per block group. Used to compute which group
    /// contains a given inode number: group = (N-1) / inodes_per_group.
    /// For our 1 MB disk: 128 (matches total inodes since we have 1 group).
    pub inodes_per_group: u32,
    /// Size of each on-disk inode in bytes. Default 128; mkfs may use
    /// 256+ for newer revisions to fit extended attributes. Used to
    /// compute byte offset within the inode table: idx * inode_size.
    pub inode_size: u16,
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
            state:              u16_at(58),
            inodes_per_group:   u32_at(40),
            inode_size:         u16_at(88),
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
        println!("[ext2]   inodes/group  = {}", self.inodes_per_group);
        println!("[ext2]   inode_size    = {} bytes", self.inode_size);
    }
}

// ============================================================
// Block Group Descriptor (Lesson 12.2)
// ============================================================

/// One entry in the Block Group Descriptor Table. 32 bytes on disk.
///
/// The BGDT is a global table that describes every block group in the
/// filesystem. Each entry tells us where that group's bitmaps and inode
/// table live. For Lesson 12, only `inode_table` matters — it gives the
/// block number where this group's inode array starts.
#[derive(Debug)]
pub struct BlockGroupDescriptor {
    /// Block number of this group's block bitmap (1 bit per data block).
    pub block_bitmap: u32,
    /// Block number of this group's inode bitmap (1 bit per inode).
    pub inode_bitmap: u32,
    /// Block number where this group's inode table starts. The load-bearing
    /// field for inode lookup.
    pub inode_table: u32,
    /// Free blocks in this group.
    pub free_blocks_count: u16,
    /// Free inodes in this group.
    pub free_inodes_count: u16,
    /// Number of directories in this group (used by the allocator to
    /// spread directories across groups for locality).
    pub used_dirs_count: u16,
}

impl BlockGroupDescriptor {
    /// Parse 32 bytes into a BlockGroupDescriptor. Per the ext2 spec.
    pub fn parse(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 32, "BGD needs 32 bytes, got {}", bytes.len());

        let u32_at = |off: usize| -> u32 {
            u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
        };
        let u16_at = |off: usize| -> u16 {
            u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap())
        };

        Self {
            block_bitmap:       u32_at(0),
            inode_bitmap:       u32_at(4),
            inode_table:        u32_at(8),
            free_blocks_count:  u16_at(12),
            free_inodes_count:  u16_at(14),
            used_dirs_count:    u16_at(16),
        }
    }

    pub fn print(&self, group_index: u32) {
        println!("[ext2] parsed block group descriptor {}:", group_index);
        println!("[ext2]   block_bitmap  = {}", self.block_bitmap);
        println!("[ext2]   inode_bitmap  = {}", self.inode_bitmap);
        println!("[ext2]   inode_table   = {}    ← load-bearing", self.inode_table);
        println!("[ext2]   free_blocks   = {}", self.free_blocks_count);
        println!("[ext2]   free_inodes   = {}", self.free_inodes_count);
        println!("[ext2]   used_dirs     = {}", self.used_dirs_count);
    }
}

// ============================================================
// Inode (Lesson 12.4)
// ============================================================

/// i_mode top-4-bit file-type masks.
pub const S_IFMT:   u16 = 0xF000;
pub const S_IFREG:  u16 = 0x8000; // regular file
pub const S_IFDIR:  u16 = 0x4000; // directory
pub const S_IFLNK:  u16 = 0xA000; // symbolic link
pub const S_IFBLK:  u16 = 0x6000; // block device
pub const S_IFCHR:  u16 = 0x2000; // character device
pub const S_IFIFO:  u16 = 0x1000; // FIFO
pub const S_IFSOCK: u16 = 0xC000; // socket

/// The parsed on-disk inode. We model the 10 fields a teaching kernel
/// needs: file type/permissions, size, link count, block count, and
/// the 15 block pointers.
#[derive(Debug)]
pub struct Inode {
    /// Packed file type (top 4 bits) + permissions (bottom 12 bits).
    pub mode: u16,
    /// Owner user ID (lower 16 bits; upper 16 in osd2 for 32-bit UIDs).
    pub uid: u16,
    /// File size in bytes.
    pub size: u32,
    /// Last-access time (Unix epoch seconds).
    pub atime: u32,
    /// Inode-change time (metadata modification).
    pub ctime: u32,
    /// Data-modification time.
    pub mtime: u32,
    /// Deletion time (0 if not deleted).
    pub dtime: u32,
    /// Owning group ID.
    pub gid: u16,
    /// Hard-link count (how many dirents point at this inode).
    pub links_count: u16,
    /// Number of 512-byte sectors used (NOT ext2 blocks).
    pub blocks: u32,
    /// 15 block pointers: 12 direct, 1 indirect, 1 double, 1 triple.
    /// We don't dereference these in Lesson 12; that's Lesson 13.
    pub block_pointers: [u32; 15],
}

impl Inode {
    /// Parse the first 128 bytes of an on-disk inode into our struct.
    /// We model the standard 128-byte struct even when s_inode_size is
    /// larger (256+); bytes 128+ are extended attributes we don't touch.
    pub fn parse(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 128, "inode needs 128 bytes, got {}", bytes.len());

        let u32_at = |off: usize| -> u32 {
            u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
        };
        let u16_at = |off: usize| -> u16 {
            u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap())
        };

        let mut block_pointers = [0u32; 15];
        for i in 0..15 {
            block_pointers[i] = u32_at(40 + i * 4);
        }

        Self {
            mode:         u16_at(0),
            uid:          u16_at(2),
            size:         u32_at(4),
            atime:        u32_at(8),
            ctime:        u32_at(12),
            mtime:        u32_at(16),
            dtime:        u32_at(20),
            gid:          u16_at(24),
            links_count:  u16_at(26),
            blocks:       u32_at(28),
            block_pointers,
        }
    }

    /// What kind of file is this? Returns the type mask (e.g., S_IFDIR).
    pub fn file_type(&self) -> u16 {
        self.mode & S_IFMT
    }

    pub fn is_dir(&self) -> bool { self.file_type() == S_IFDIR }
    pub fn is_reg(&self) -> bool { self.file_type() == S_IFREG }
    pub fn is_lnk(&self) -> bool { self.file_type() == S_IFLNK }

    /// Human-readable file type label.
    pub fn type_name(&self) -> &'static str {
        match self.file_type() {
            S_IFREG  => "regular file",
            S_IFDIR  => "directory",
            S_IFLNK  => "symlink",
            S_IFBLK  => "block device",
            S_IFCHR  => "char device",
            S_IFIFO  => "fifo",
            S_IFSOCK => "socket",
            _        => "unknown",
        }
    }

    pub fn print(&self, inode_num: u32) {
        println!("[ext2] parsed inode {}:", inode_num);
        println!(
            "[ext2]   mode          = 0x{:04X}  ({}, perms 0o{:o})",
            self.mode, self.type_name(), self.mode & 0o7777
        );
        println!("[ext2]   uid:gid       = {}:{}", self.uid, self.gid);
        println!("[ext2]   size          = {} bytes", self.size);
        println!("[ext2]   blocks        = {} sectors ({} ext2 blocks)",
            self.blocks, self.blocks / 2);
        println!("[ext2]   links_count   = {}", self.links_count);
        let direct: Vec<u32> = self.block_pointers.iter().take(12)
            .filter(|&&b| b != 0).copied().collect();
        println!("[ext2]   block_ptrs    = direct {:?}, indirect={}, double={}, triple={}",
            direct, self.block_pointers[12], self.block_pointers[13], self.block_pointers[14]);
    }
}

/// Look up inode `inode_num` on disk and return its parsed struct.
///
/// Arithmetic:
///   group      = (inode_num - 1) / inodes_per_group
///   local_idx  = (inode_num - 1) % inodes_per_group
///   byte_offset = bg_inode_table[group] * block_size + local_idx * inode_size
///
/// For inode 2 on our disk: group 0, local_idx 1, bg_inode_table = 5,
/// block_size 1024, inode_size 256. Byte offset = 5*1024 + 1*256 = 5376.
pub fn read_inode(
    dev: &mut VirtioBlk,
    sb: &Superblock,
    bgd: &BlockGroupDescriptor,
    inode_num: u32,
) -> Inode {
    assert!(inode_num >= 1, "inode numbers are 1-indexed");

    let local_idx = (inode_num - 1) % sb.inodes_per_group;
    let inode_byte = bgd.inode_table as u64 * sb.block_size as u64
        + local_idx as u64 * sb.inode_size as u64;

    println!(
        "[ext2] reading inode {} (group 0, index {}, byte {})",
        inode_num, local_idx, inode_byte
    );

    // The 128-byte inode may straddle a sector boundary. Read the two
    // sectors that contain it, then index in.
    let start_sector = inode_byte / 512;
    let offset_in_buf = (inode_byte % 512) as usize;
    let sector_a = dev.read_sector(start_sector);
    let sector_b = dev.read_sector(start_sector + 1);
    let mut buf = [0u8; 1024];
    buf[..512].copy_from_slice(&sector_a);
    buf[512..].copy_from_slice(&sector_b);

    Inode::parse(&buf[offset_in_buf..offset_in_buf + 128])
}

// ============================================================
// Block read helper (Lesson 13.2)
// ============================================================

/// Read one ext2 block (1024 bytes = 2 sectors). Used by superblock,
/// inode, and now directory readers. Assumes 1024-byte blocks — our
/// disk's only size. A block-size-aware version is a future refactor.
pub fn read_block(dev: &mut VirtioBlk, block_n: u32) -> [u8; 1024] {
    let sector_a = dev.read_sector((block_n as u64) * 2);
    let sector_b = dev.read_sector((block_n as u64) * 2 + 1);
    let mut buf = [0u8; 1024];
    buf[..512].copy_from_slice(&sector_a);
    buf[512..].copy_from_slice(&sector_b);
    buf
}

// ============================================================
// Directory entry (Lesson 13.1)
// ============================================================

/// ext2 file-type bits stored in the dirent's `file_type` byte.
/// Same encoding as i_mode's top 4 bits, just shifted right by 12.
pub const EXT_FT_UNKNOWN:  u8 = 0;
pub const EXT_FT_REG_FILE: u8 = 1;
pub const EXT_FT_DIR:      u8 = 2;
pub const EXT_FT_CHRDEV:   u8 = 3;
pub const EXT_FT_BLKDEV:   u8 = 4;
pub const EXT_FT_FIFO:     u8 = 5;
pub const EXT_FT_SOCK:     u8 = 6;
pub const EXT_FT_SYMLINK:  u8 = 7;

/// A parsed directory entry. The name is owned (heap-allocated Vec)
/// so callers don't have to deal with byte slices + length tracking.
#[derive(Debug)]
pub struct Dirent {
    /// Inode number this entry points at. 0 means the slot is dead
    /// (deleted but space not reclaimed).
    pub inode: u32,
    /// Bytes to skip to reach the next entry (linked list).
    pub rec_len: u16,
    /// Length of the name in bytes (no null terminator on disk).
    pub name_len: u8,
    /// File-type hint so walkers don't have to read the inode just
    /// to know if it's a directory. See EXT_FT_* constants.
    pub file_type: u8,
    /// The actual name bytes.
    pub name: Vec<u8>,
}

impl Dirent {
    /// Human-readable type label.
    pub fn type_name(&self) -> &'static str {
        match self.file_type {
            EXT_FT_REG_FILE => "file",
            EXT_FT_DIR      => "dir",
            EXT_FT_CHRDEV   => "char-dev",
            EXT_FT_BLKDEV   => "block-dev",
            EXT_FT_FIFO     => "fifo",
            EXT_FT_SOCK     => "sock",
            EXT_FT_SYMLINK  => "symlink",
            _               => "unknown",
        }
    }

    /// True if this entry is live (inode != 0). Dead entries still
    /// occupy space — the walker skips them but advances by rec_len.
    pub fn is_live(&self) -> bool {
        self.inode != 0
    }

    /// Parse the dirent at `offset` within `bytes`. Returns the parsed
    /// Dirent plus the offset of the NEXT entry (offset + rec_len).
    ///
    /// The returned Dirent is always populated (even for dead entries)
    /// so the walker can decide what to do with them.
    pub fn parse_at(bytes: &[u8], offset: usize) -> (Dirent, usize) {
        let u32_at = |off: usize| -> u32 {
            u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
        };
        let u16_at = |off: usize| -> u16 {
            u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap())
        };
        let u8_at = |off: usize| -> u8 { bytes[off] };

        let inode = u32_at(offset);
        let rec_len = u16_at(offset + 4);
        let name_len = u8_at(offset + 6);
        let file_type = u8_at(offset + 7);

        // Name bytes start at offset+8 and run for name_len bytes.
        // Spec guarantees no null terminator; we copy as-is.
        let name_start = offset + 8;
        let name_end = name_start + name_len as usize;
        let name = if name_end <= bytes.len() {
            bytes[name_start..name_end].to_vec()
        } else {
            // Truncated entry — shouldn't happen on a valid filesystem.
            Vec::new()
        };

        let next_offset = offset + rec_len as usize;
        (Dirent { inode, rec_len, name_len, file_type, name }, next_offset)
    }
}

// ============================================================
// Directory walk (Lesson 13.3 + 13.4)
// ============================================================

/// Walk a directory's entries, returning every live dirent.
///
/// Reads each direct block (block_pointers[0..12]) that's non-zero,
/// parses the linked list of entries inside, and collects those with
/// `inode != 0`. Doesn't follow indirect/double/triple pointers —
/// that's only needed for directories larger than 12 KiB, which ours
/// isn't.
pub fn walk_directory(dev: &mut VirtioBlk, dir_inode: &Inode) -> Vec<Dirent> {
    let mut entries = Vec::new();

    for (i, &block_n) in dir_inode.block_pointers.iter().enumerate().take(12) {
        if block_n == 0 {
            continue;
        }
        println!("[ext2]   scanning direct block {} (block #{})", i, block_n);
        let block = read_block(dev, block_n);

        let mut offset = 0usize;
        while offset + 8 <= block.len() {
            let (entry, next_offset) = Dirent::parse_at(&block, offset);
            if entry.is_live() {
                let name_str = core::str::from_utf8(&entry.name).unwrap_or("<non-utf8>");
                println!(
                    "[ext2]     entry: inode={:<5} type={} name=\"{}\"",
                    entry.inode, entry.type_name(), name_str
                );
                entries.push(entry);
            }
            if next_offset <= offset {
                // rec_len of 0 would loop forever; bail out.
                break;
            }
            offset = next_offset;
        }
    }

    entries
}

/// Look up `name` in a directory's entries. Returns the matching
/// inode number, or None if not found. This is the path-resolution
/// primitive: "/foo" becomes walk root → lookup_in_dir("foo").
pub fn lookup_in_dir(dev: &mut VirtioBlk, dir_inode: &Inode, name: &[u8]) -> Option<u32> {
    let entries = walk_directory(dev, dir_inode);
    for entry in entries {
        if entry.name == name {
            return Some(entry.inode);
        }
    }
    None
}
///
/// The BGDT lives in the block right after the superblock. For 1024-byte
/// block filesystems, that's block 2 (superblock is block 1). Each entry
/// is 32 bytes; entry N is at byte offset N * 32 within the BGDT block.
///
/// For our 1 MB disk with one block group, only `group_index = 0`
/// makes sense — but the math generalizes to multi-group disks.
pub fn read_block_group_descriptor(
    dev: &mut VirtioBlk,
    sb: &Superblock,
    group_index: u32,
) -> BlockGroupDescriptor {
    let bgdt_block = sb.first_data_block + 1;
    let bgdt_byte = bgdt_block as u64 * sb.block_size as u64;
    let entry_byte = bgdt_byte + (group_index as u64) * 32;

    println!(
        "[ext2] reading block group descriptor {} (BGDT block {}, byte {})",
        group_index, bgdt_block, entry_byte
    );

    // The 32-byte entry may straddle a sector boundary. For our disk
    // (entry at byte 2048, sectors are 512 B), it doesn't. Read the
    // containing sector, index into it.
    let sector = (entry_byte / 512) as u64;
    let offset_in_sector = (entry_byte % 512) as usize;
    let sector_bytes = dev.read_sector(sector);

    BlockGroupDescriptor::parse(&sector_bytes[offset_in_sector..offset_in_sector + 32])
}
