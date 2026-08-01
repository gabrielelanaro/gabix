use crate::device::BlockDevice;
use alloc::vec::Vec;

const EXT2_MAGIC: u16 = 0xef53;
const ROOT_INODE: u32 = 2;
const DIRECTORY_TYPE: u16 = 0x4000;
const REGULAR_TYPE: u16 = 0x8000;
const TYPE_MASK: u16 = 0xf000;

struct Superblock {
    first_data_block: u32,
    block_size: u32,
    inodes_per_group: u32,
    inode_size: u16,
}

struct Inode {
    mode: u16,
    size: u32,
    blocks: [u32; 15],
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
/// filesystem and user programs up to 12 filesystem blocks.
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
    let first = device.read_sector(2);
    let second = device.read_sector(3);
    let mut bytes = [0u8; 1024];
    bytes[..512].copy_from_slice(&first);
    bytes[512..].copy_from_slice(&second);

    if read_u16(&bytes, 56) != EXT2_MAGIC {
        return None;
    }

    Some(Superblock {
        first_data_block: read_u32(&bytes, 20),
        block_size: 1024 << read_u32(&bytes, 24),
        inodes_per_group: read_u32(&bytes, 40),
        inode_size: read_u16(&bytes, 88),
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

    let group = (inode_number - 1) / superblock.inodes_per_group;
    let local_index = (inode_number - 1) % superblock.inodes_per_group;
    let descriptor_offset =
        (superblock.first_data_block + 1) as u64 * superblock.block_size as u64 + group as u64 * 32;
    let descriptor = read_bytes(device, descriptor_offset, 32);
    let inode_table = read_u32(&descriptor, 8);

    let inode_offset = inode_table as u64 * superblock.block_size as u64
        + local_index as u64 * superblock.inode_size as u64;
    let bytes = read_bytes(device, inode_offset, 128);

    let mut blocks = [0u32; 15];
    for (index, block) in blocks.iter_mut().enumerate() {
        *block = read_u32(&bytes, 40 + index * 4);
    }
    Some(Inode {
        mode: read_u16(&bytes, 0),
        size: read_u32(&bytes, 4),
        blocks,
    })
}

fn lookup(
    device: &mut dyn BlockDevice,
    superblock: &Superblock,
    directory: &Inode,
    name: &[u8],
) -> Option<u32> {
    for &block_number in directory.blocks.iter().take(12) {
        if block_number == 0 {
            continue;
        }
        let block = read_block(device, superblock, block_number);
        let mut offset = 0usize;
        while offset + 8 <= block.len() {
            let inode = read_u32(&block, offset);
            let record_len = read_u16(&block, offset + 4) as usize;
            let name_len = block[offset + 6] as usize;
            if record_len < 8 || offset + record_len > block.len() {
                break;
            }
            let entry_name = &block[offset + 8..offset + 8 + name_len.min(record_len - 8)];
            if inode != 0 && bytes_equal(entry_name, name) {
                return Some(inode);
            }
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
    for &block_number in inode.blocks.iter().take(12) {
        if block_number == 0 {
            break;
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
    let first_sector = offset / 512;
    let end = offset + len as u64;
    let final_sector = end.div_ceil(512);

    for sector_number in first_sector..final_sector {
        let sector = device.read_sector(sector_number);
        let start = if sector_number == first_sector {
            (offset % 512) as usize
        } else {
            0
        };
        let remaining = len - output.len();
        let count = remaining.min(512 - start);
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
