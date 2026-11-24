//! Small read-only ext2 reader for the boot filesystem.
//! Directory entries map names to inode numbers. Inodes supply file type, size,
//! and block pointers; this reader follows only the twelve direct pointers.
//! Disk layouts: Linux fs/ext2/ext2.h, ext2_super_block/ext2_inode/ext2_dir_entry_2.

use crate::device::BlockDevice;
use alloc::vec::Vec;

const EXT2_MAGIC: u16 = 0xef53;
const ROOT_INODE: u32 = 2;
const DIRECTORY_TYPE: u16 = 0x4000;
const REGULAR_TYPE: u16 = 0x8000;
const TYPE_MASK: u16 = 0xf000;

const SECTOR_BYTES: usize = 512;
const SUPERBLOCK_BYTES: usize = 1024;
const SUPERBLOCK_FIRST_SECTOR: u64 = 2;
const MIN_BLOCK_BYTES: u32 = 1024;
const DIRECT_BLOCK_COUNT: usize = 12;
const INODE_BLOCK_POINTER_COUNT: usize = 15;
const BLOCK_POINTER_BYTES: usize = 4;
const GROUP_DESCRIPTOR_BYTES: usize = 32;
const GROUP_INODE_TABLE_OFFSET: usize = 8;
const BASE_INODE_BYTES: usize = 128;

mod superblock_field {
    pub const FIRST_DATA_BLOCK: usize = 20;
    pub const LOG_BLOCK_SIZE: usize = 24;
    pub const INODES_PER_GROUP: usize = 40;
    pub const MAGIC: usize = 56;
    pub const INODE_SIZE: usize = 88;
}

mod inode_field {
    pub const MODE: usize = 0;
    pub const SIZE: usize = 4;
    pub const BLOCK_POINTERS: usize = 40;
}

mod directory_entry {
    pub const HEADER_BYTES: usize = 8;
    pub const RECORD_LENGTH: usize = 4;
    pub const NAME_LENGTH: usize = 6;
}

struct Superblock {
    first_data_block: u32,
    block_size: u32,
    inodes_per_group: u32,
    inode_size: u16,
}

struct Inode {
    mode: u16,
    size: u32,
    blocks: [u32; INODE_BLOCK_POINTER_COUNT],
}

impl Inode {
    fn is_directory(&self) -> bool {
        self.mode & TYPE_MASK == DIRECTORY_TYPE
    }

    fn is_regular(&self) -> bool {
        self.mode & TYPE_MASK == REGULAR_TYPE
    }
}

/// Resolve an absolute path and return its bytes from an ext2 block device.
///
/// The reader supports direct block pointers, which covers Gabix's small boot
/// filesystem and user programs up to 12 filesystem blocks. Larger files
/// currently return only that prefix; indirect blocks are not followed.
/// This assumes the image format produced by our disk-building scripts and
/// does not fully validate arbitrary or corrupt ext2 images.
pub fn read_file(device: &mut dyn BlockDevice, path: &[u8]) -> Option<Vec<u8>> {
    let superblock = read_superblock(device)?;
    let mut inode_number = ROOT_INODE;

    for component in path.split(|byte| *byte == b'/') {
        if component.is_empty() {
            continue;
        }
        let directory = read_inode(device, &superblock, inode_number)?;
        if !directory.is_directory() {
            return None;
        }
        inode_number = lookup(device, &superblock, &directory, component)?;
    }

    let inode = read_inode(device, &superblock, inode_number)?;
    if !inode.is_regular() {
        return None;
    }
    Some(read_inode_data(device, &superblock, &inode))
}

fn read_superblock(device: &mut dyn BlockDevice) -> Option<Superblock> {
    let first = device.read_sector(SUPERBLOCK_FIRST_SECTOR);
    let second = device.read_sector(SUPERBLOCK_FIRST_SECTOR + 1);
    let mut bytes = [0u8; SUPERBLOCK_BYTES];
    bytes[..SECTOR_BYTES].copy_from_slice(&first);
    bytes[SECTOR_BYTES..].copy_from_slice(&second);

    if read_u16(&bytes, superblock_field::MAGIC) != EXT2_MAGIC {
        return None;
    }

    Some(Superblock {
        first_data_block: read_u32(&bytes, superblock_field::FIRST_DATA_BLOCK),
        block_size: MIN_BLOCK_BYTES << read_u32(&bytes, superblock_field::LOG_BLOCK_SIZE),
        inodes_per_group: read_u32(&bytes, superblock_field::INODES_PER_GROUP),
        inode_size: read_u16(&bytes, superblock_field::INODE_SIZE),
    })
}

fn read_inode(
    device: &mut dyn BlockDevice,
    superblock: &Superblock,
    inode_number: u32,
) -> Option<Inode> {
    if inode_number == 0 || superblock.inodes_per_group == 0 {
        return None;
    }

    // Inode numbers start at one. Convert to a zero-based group and table slot.
    let group = (inode_number - 1) / superblock.inodes_per_group;
    let local_index = (inode_number - 1) % superblock.inodes_per_group;
    // The primary group-descriptor table follows the block containing the
    // superblock; each descriptor identifies its group's inode-table block.
    let descriptor_offset = (superblock.first_data_block + 1) as u64 * superblock.block_size as u64
        + group as u64 * GROUP_DESCRIPTOR_BYTES as u64;
    let descriptor = read_bytes(device, descriptor_offset, GROUP_DESCRIPTOR_BYTES);
    let inode_table = read_u32(&descriptor, GROUP_INODE_TABLE_OFFSET);

    let inode_offset = inode_table as u64 * superblock.block_size as u64
        + local_index as u64 * superblock.inode_size as u64;
    let bytes = read_bytes(device, inode_offset, BASE_INODE_BYTES);

    let mut blocks = [0u32; INODE_BLOCK_POINTER_COUNT];
    for (index, block) in blocks.iter_mut().enumerate() {
        *block = read_u32(
            &bytes,
            inode_field::BLOCK_POINTERS + index * BLOCK_POINTER_BYTES,
        );
    }
    Some(Inode {
        mode: read_u16(&bytes, inode_field::MODE),
        size: read_u32(&bytes, inode_field::SIZE),
        blocks,
    })
}

fn lookup(
    device: &mut dyn BlockDevice,
    superblock: &Superblock,
    directory: &Inode,
    name: &[u8],
) -> Option<u32> {
    for &block_number in directory.blocks.iter().take(DIRECT_BLOCK_COUNT) {
        if block_number == 0 {
            continue;
        }
        let block = read_block(device, superblock, block_number);
        let mut offset = 0usize;
        while offset + directory_entry::HEADER_BYTES <= block.len() {
            let inode = read_u32(&block, offset);
            let record_len = read_u16(&block, offset + directory_entry::RECORD_LENGTH) as usize;
            let name_len = block[offset + directory_entry::NAME_LENGTH] as usize;
            if record_len < directory_entry::HEADER_BYTES || offset + record_len > block.len() {
                break;
            }
            let name_start = offset + directory_entry::HEADER_BYTES;
            let available_name_bytes = record_len - directory_entry::HEADER_BYTES;
            let entry_name = &block[name_start..name_start + name_len.min(available_name_bytes)];
            if inode != 0 && bytes_equal(entry_name, name) {
                return Some(inode);
            }
            // Records include padding; advance by record length, not name length.
            offset += record_len;
        }
    }
    None
}

fn read_inode_data(
    device: &mut dyn BlockDevice,
    superblock: &Superblock,
    inode: &Inode,
) -> Vec<u8> {
    let mut data = Vec::new();
    for &block_number in inode.blocks.iter().take(DIRECT_BLOCK_COUNT) {
        if data.len() >= inode.size as usize {
            break;
        }
        if block_number == 0 {
            // A zero direct pointer represents a hole: return zero-filled bytes.
            data.resize(data.len() + superblock.block_size as usize, 0);
            continue;
        }
        let block = read_block(device, superblock, block_number);
        data.extend_from_slice(&block);
    }
    data.truncate(inode.size as usize);
    data
}

fn read_block(device: &mut dyn BlockDevice, superblock: &Superblock, block_number: u32) -> Vec<u8> {
    read_bytes(
        device,
        block_number as u64 * superblock.block_size as u64,
        superblock.block_size as usize,
    )
}

fn read_bytes(device: &mut dyn BlockDevice, offset: u64, len: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(len);
    let first_sector = offset / SECTOR_BYTES as u64;
    let end = offset + len as u64;
    let final_sector = end.div_ceil(SECTOR_BYTES as u64);

    for sector_number in first_sector..final_sector {
        let sector = device.read_sector(sector_number);
        let start = if sector_number == first_sector {
            (offset % SECTOR_BYTES as u64) as usize
        } else {
            0
        };
        let remaining = len - output.len();
        let count = remaining.min(SECTOR_BYTES - start);
        output.extend_from_slice(&sector[start..start + count]);
    }
    output
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn bytes_equal(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(left, right)| left == right)
}
