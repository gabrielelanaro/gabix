//! Read the four primary partition entries in a Master Boot Record (MBR).
//! A partition start is a sector number relative to the whole disk. The adapter
//! below adds that start to filesystem-relative sector numbers.
//! Layout reference: UEFI specification 2.10, section 5.2.1, tables 5.1–5.2.

use crate::device::BlockDevice;

const MBR_SECTOR: u64 = 0;
const MBR_SIGNATURE_OFFSET: usize = 510;
const MBR_SIGNATURE: [u8; 2] = [0x55, 0xaa];
const PRIMARY_PARTITION_COUNT: usize = 4;
const START_SECTOR_BYTES: usize = core::mem::size_of::<u32>();
const PARTITION_TABLE_OFFSET: usize = 446;
const PARTITION_ENTRY_LEN: usize = 16;
const PARTITION_TYPE_OFFSET: usize = 4;
const PARTITION_START_OFFSET: usize = 8;

/// Find the first matching primary entry with a nonzero starting sector.
/// This lookup checks the MBR signature, but does not validate partition lengths,
/// overlaps, or filesystem contents. It does not traverse extended partitions.
pub fn mbr_partition_start(device: &mut impl BlockDevice, partition_type: u8) -> Option<u64> {
    let mbr = device.read_sector(MBR_SECTOR);
    if mbr[MBR_SIGNATURE_OFFSET..MBR_SIGNATURE_OFFSET + MBR_SIGNATURE.len()] != MBR_SIGNATURE {
        return None;
    }
    for index in 0..PRIMARY_PARTITION_COUNT {
        let entry = PARTITION_TABLE_OFFSET + index * PARTITION_ENTRY_LEN;
        if mbr[entry + PARTITION_TYPE_OFFSET] != partition_type {
            continue;
        }
        // The entry stores a little-endian 32-bit logical block address (LBA).
        let start = u32::from_le_bytes(
            mbr[entry + PARTITION_START_OFFSET
                ..entry + PARTITION_START_OFFSET + START_SECTOR_BYTES]
                .try_into()
                .ok()?,
        );
        if start != 0 {
            return Some(start as u64);
        }
    }
    None
}

/// Translate filesystem sector numbers to whole-device sector numbers.
/// This adapter stores no partition length, so it does not enforce an upper
/// boundary; callers must validate their reads against the filesystem layout.
pub struct OffsetBlockDevice<'a, D> {
    device: &'a mut D,
    start_sector: u64,
}

impl<'a, D> OffsetBlockDevice<'a, D> {
    pub fn new(device: &'a mut D, start_sector: u64) -> Self {
        Self {
            device,
            start_sector,
        }
    }
}

unsafe impl<D: Send> Send for OffsetBlockDevice<'_, D> {}

impl<D: BlockDevice> BlockDevice for OffsetBlockDevice<'_, D> {
    fn read_sector(&mut self, sector: u64) -> [u8; 512] {
        // This adapter returns zeros on address overflow. BlockDevice has no
        // error return, so the caller cannot distinguish this from zeroed data.
        let Some(sector) = self.start_sector.checked_add(sector) else {
            return [0; 512];
        };
        self.device.read_sector(sector)
    }
}
