use crate::device::BlockDevice;

const PARTITION_TABLE_OFFSET: usize = 446;
const PARTITION_ENTRY_LEN: usize = 16;
const PARTITION_TYPE_OFFSET: usize = 4;
const PARTITION_START_OFFSET: usize = 8;

pub fn mbr_partition_start(device: &mut impl BlockDevice, partition_type: u8) -> Option<u64> {
    let mbr = device.read_sector(0);
    if mbr[510..512] != [0x55, 0xaa] {
        return None;
    }
    for index in 0..4 {
        let entry = PARTITION_TABLE_OFFSET + index * PARTITION_ENTRY_LEN;
        if mbr[entry + PARTITION_TYPE_OFFSET] != partition_type {
            continue;
        }
        let start = u32::from_le_bytes(
            mbr[entry + PARTITION_START_OFFSET..entry + PARTITION_START_OFFSET + 4]
                .try_into()
                .ok()?,
        );
        if start != 0 {
            return Some(start as u64);
        }
    }
    None
}

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
        let Some(sector) = self.start_sector.checked_add(sector) else {
            return [0; 512];
        };
        self.device.read_sector(sector)
    }
}
