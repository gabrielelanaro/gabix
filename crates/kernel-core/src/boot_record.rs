//! The SD loader's single, checksummed boot decision and kernel extents.
//! Extents are relative to the kernel partition, so relocation changes the MBR.
const SECTOR_BYTES: usize = 512;
const FORMAT_VERSION: u32 = 1;
const COPY_COUNT: usize = 2;
const ALL_COPIES_ATTEMPTED: u32 = (1 << COPY_COUNT) - 1;
const CRC32_REFLECTED_POLYNOMIAL: u32 = 0xedb8_8320;

// Byte offsets in the single-sector record. Every integer is little-endian.
mod field {
    pub const VERSION: usize = 8;
    pub const ACTIVE_COPY: usize = 12;
    pub const COPIES: [usize; 2] = [16, 40];
    pub const PENDING_COPY: usize = 64;
    pub const ATTEMPTED_MASK: usize = 68;
    pub const RUNNING_COPY: usize = 72;
    pub const CHECKSUM: usize = 508;
}

// Each copy descriptor occupies 24 bytes: two u64 sector counts and two u32s.
mod copy_field {
    pub const BYTES: usize = 24;
    pub const CAPACITY: usize = 8;
    pub const IMAGE_BYTES: usize = 16;
    pub const IMAGE_CRC: usize = 20;
}

// MBR primary entries, as in UEFI 2.10 section 5.2.1. This module is also
// built without the filesystem feature for the resident loader.
mod mbr_field {
    pub const TABLE: core::ops::Range<usize> = 446..510;
    pub const SIGNATURE_OFFSET: usize = 510;
    pub const SIGNATURE: [u8; 2] = [0x55, 0xaa];
    pub const ENTRY_BYTES: usize = 16;
    pub const PARTITION_TYPE: usize = 4;
    pub const START_SECTOR: usize = 8;
    pub const SECTOR_COUNT: usize = 12;
}

pub const MAGIC: &[u8; 8] = b"GABIXBT1";
pub const PARTITION_TYPE: u8 = 0xda;
pub const NO_COPY: u32 = u32::MAX;
pub const HANDOFF_ADDRESS: usize = 0x0006_0000;
pub const HANDOFF_MAGIC: u32 = 0x4742_5848;

/// Little-endian RAM message from the SD loader to the running kernel.
/// This describes the attempted image; the persistent boot record stays on SD.
pub mod handoff_field {
    pub const BYTE_LEN: usize = 28;
    pub const MAGIC: core::ops::Range<usize> = 0..4;
    pub const RECORD_SECTOR: core::ops::Range<usize> = 4..12;
    pub const COPY_INDEX: core::ops::Range<usize> = 12..16;
    pub const IMAGE_LENGTH: core::ops::Range<usize> = 16..20;
    pub const IMAGE_CRC: core::ops::Range<usize> = 20..24;
    pub const CHECKSUM: core::ops::Range<usize> = 24..28;
    pub const CHECKSUM_INPUT: core::ops::Range<usize> = 0..24;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelCopy {
    pub offset_sectors: u64,
    pub capacity_sectors: u64,
    pub image_bytes: u32,
    pub image_crc32: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootRecord {
    /// Last confirmed kernel, retained while a replacement is on trial.
    pub active_copy: usize,
    pub copies: [KernelCopy; COPY_COUNT],
    /// Candidate tried first, or NO_COPY when no update is pending.
    pub pending_copy: u32,
    /// Bit i records that copy i has already been attempted since confirmation.
    pub attempted_mask: u32,
    /// Most recently selected copy; confirmation must identify this copy.
    pub running_copy: u32,
}

impl BootRecord {
    pub fn decode(bytes: &[u8; SECTOR_BYTES], partition_sectors: u64) -> Option<Self> {
        if &bytes[..MAGIC.len()] != MAGIC
            || u32_at(bytes, field::VERSION) != FORMAT_VERSION
            || crc32(&bytes[..field::CHECKSUM]) != u32_at(bytes, field::CHECKSUM)
        {
            return None;
        }
        let active_copy = u32_at(bytes, field::ACTIVE_COPY) as usize;
        if active_copy > 1 {
            return None;
        }
        let copies = [
            copy_at(bytes, field::COPIES[0]),
            copy_at(bytes, field::COPIES[1]),
        ];
        for copy in copies {
            if copy.offset_sectors == 0
                || copy.capacity_sectors == 0
                || copy.offset_sectors.checked_add(copy.capacity_sectors)? > partition_sectors
                || copy.image_bytes as u64
                    > copy.capacity_sectors.checked_mul(SECTOR_BYTES as u64)?
            {
                return None;
            }
        }
        if copies[0].offset_sectors < copies[1].offset_sectors + copies[1].capacity_sectors
            && copies[1].offset_sectors < copies[0].offset_sectors + copies[0].capacity_sectors
        {
            return None;
        }
        if copies[active_copy].image_bytes == 0 {
            return None;
        }
        let pending_copy = u32_at(bytes, field::PENDING_COPY);
        let attempted_mask = u32_at(bytes, field::ATTEMPTED_MASK);
        let running_copy = u32_at(bytes, field::RUNNING_COPY);
        if (pending_copy > 1 && pending_copy != NO_COPY)
            || attempted_mask > ALL_COPIES_ATTEMPTED
            || (running_copy > 1 && running_copy != NO_COPY)
        {
            return None;
        }
        Some(Self {
            active_copy,
            copies,
            pending_copy,
            attempted_mask,
            running_copy,
        })
    }

    pub fn encode(&self) -> [u8; SECTOR_BYTES] {
        let mut bytes = [0; SECTOR_BYTES];
        bytes[..MAGIC.len()].copy_from_slice(MAGIC);
        bytes[field::VERSION..field::VERSION + 4].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes[field::ACTIVE_COPY..field::ACTIVE_COPY + 4]
            .copy_from_slice(&(self.active_copy as u32).to_le_bytes());
        for (offset, copy) in [
            (field::COPIES[0], self.copies[0]),
            (field::COPIES[1], self.copies[1]),
        ] {
            bytes[offset..offset + copy_field::CAPACITY]
                .copy_from_slice(&copy.offset_sectors.to_le_bytes());
            bytes[offset + copy_field::CAPACITY..offset + copy_field::IMAGE_BYTES]
                .copy_from_slice(&copy.capacity_sectors.to_le_bytes());
            bytes[offset + copy_field::IMAGE_BYTES..offset + copy_field::IMAGE_CRC]
                .copy_from_slice(&copy.image_bytes.to_le_bytes());
            bytes[offset + copy_field::IMAGE_CRC..offset + copy_field::BYTES]
                .copy_from_slice(&copy.image_crc32.to_le_bytes());
        }
        bytes[field::PENDING_COPY..field::PENDING_COPY + 4]
            .copy_from_slice(&self.pending_copy.to_le_bytes());
        bytes[field::ATTEMPTED_MASK..field::ATTEMPTED_MASK + 4]
            .copy_from_slice(&self.attempted_mask.to_le_bytes());
        bytes[field::RUNNING_COPY..field::RUNNING_COPY + 4]
            .copy_from_slice(&self.running_copy.to_le_bytes());
        let crc = crc32(&bytes[..field::CHECKSUM]);
        bytes[field::CHECKSUM..].copy_from_slice(&crc.to_le_bytes());
        bytes
    }

    /// Select pending, then confirmed, then the other copy, skipping attempts.
    /// This updates only the in-memory record. The loader must persist it before
    /// executing the image so a reset advances to the remaining copy.
    pub fn next_attempt(&mut self) -> Option<usize> {
        for index in [
            self.pending_copy,
            self.active_copy as u32,
            (1 - self.active_copy) as u32,
        ] {
            if index <= 1
                && self.attempted_mask & (1 << index) == 0
                && self.copies[index as usize].image_bytes != 0
            {
                self.attempted_mask |= 1 << index;
                self.running_copy = index;
                return Some(index as usize);
            }
        }
        None
    }

    /// Promote the selected attempt and clear trial state in memory. The caller
    /// must write the encoded record to make confirmation survive a reset.
    pub fn confirm(&mut self, index: u32) -> bool {
        if index > 1 || self.running_copy != index || self.attempted_mask & (1 << index) == 0 {
            return false;
        }
        self.active_copy = index as usize;
        self.pending_copy = NO_COPY;
        self.running_copy = NO_COPY;
        self.attempted_mask = 0;
        true
    }
}

/// Return the unique kernel partition only when all occupied extents fit and
/// none overlap it. The caller reads the boot record from its first sector.
pub fn find_partition(mbr: &[u8; SECTOR_BYTES], card_sectors: u64) -> Option<(u64, u64)> {
    if mbr[mbr_field::SIGNATURE_OFFSET..] != mbr_field::SIGNATURE {
        return None;
    }
    let mut found = None;
    for entry in mbr[mbr_field::TABLE].chunks_exact(mbr_field::ENTRY_BYTES) {
        if entry[mbr_field::PARTITION_TYPE] == PARTITION_TYPE {
            if found.is_some() {
                return None;
            }
            found = Some((
                u32_at(entry, mbr_field::START_SECTOR) as u64,
                u32_at(entry, mbr_field::SECTOR_COUNT) as u64,
            ));
        }
    }
    let (start, count) = found?;
    if start == 0 || count == 0 || start.checked_add(count)? > card_sectors {
        return None;
    }
    for entry in mbr[mbr_field::TABLE].chunks_exact(mbr_field::ENTRY_BYTES) {
        if entry[mbr_field::PARTITION_TYPE] == 0 {
            continue;
        }
        let other_start = u32_at(entry, mbr_field::START_SECTOR) as u64;
        let other_count = u32_at(entry, mbr_field::SECTOR_COUNT) as u64;
        let other_end = other_start.checked_add(other_count)?;
        if other_start == 0 || other_count == 0 || other_end > card_sectors {
            return None;
        }
        if entry[mbr_field::PARTITION_TYPE] != PARTITION_TYPE
            && start < other_end
            && other_start < start + count
        {
            return None;
        }
    }
    Some((start, count))
}

/// Extend the running CRC state. Start with u32::MAX and complement once after
/// the last chunk; crc32 supplies those steps for a complete byte slice.
pub fn update_crc(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (CRC32_REFLECTED_POLYNOMIAL & 0u32.wrapping_sub(crc & 1));
        }
    }
    crc
}
pub fn crc32(bytes: &[u8]) -> u32 {
    !update_crc(!0, bytes)
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn copy_at(bytes: &[u8], offset: usize) -> KernelCopy {
    KernelCopy {
        offset_sectors: u64::from_le_bytes(
            bytes[offset..offset + copy_field::CAPACITY]
                .try_into()
                .unwrap(),
        ),
        capacity_sectors: u64::from_le_bytes(
            bytes[offset + copy_field::CAPACITY..offset + copy_field::IMAGE_BYTES]
                .try_into()
                .unwrap(),
        ),
        image_bytes: u32_at(bytes, offset + copy_field::IMAGE_BYTES),
        image_crc32: u32_at(bytes, offset + copy_field::IMAGE_CRC),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> [u8; 512] {
        let mut bytes = [0; 512];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        for (offset, start) in [(16, 2048u64), (40, 67584u64)] {
            bytes[offset..offset + 8].copy_from_slice(&start.to_le_bytes());
            bytes[offset + 8..offset + 16].copy_from_slice(&65536u64.to_le_bytes());
            bytes[offset + 16..offset + 20].copy_from_slice(&68028u32.to_le_bytes());
        }
        bytes[64..68].copy_from_slice(&NO_COPY.to_le_bytes());
        bytes[72..76].copy_from_slice(&NO_COPY.to_le_bytes());
        seal(&mut bytes);
        bytes
    }
    fn seal(bytes: &mut [u8; 512]) {
        let crc = crc32(&bytes[..508]);
        bytes[508..].copy_from_slice(&crc.to_le_bytes());
    }
    #[test]
    fn test_record_rejects_corruption_and_unsafe_extents() {
        let good = record();
        assert_eq!(
            BootRecord::decode(&good, 262144).unwrap().copies[1].capacity_sectors,
            65536
        );
        let mut bad = good;
        bad[100] ^= 1;
        assert!(BootRecord::decode(&bad, 262144).is_none());
        let mut bad = good;
        bad[40..48].copy_from_slice(&2048u64.to_le_bytes());
        seal(&mut bad);
        assert!(BootRecord::decode(&bad, 262144).is_none());
        assert!(BootRecord::decode(&good, 100000).is_none());
        let mut bad = good;
        bad[12..16].copy_from_slice(&2u32.to_le_bytes());
        seal(&mut bad);
        assert!(BootRecord::decode(&bad, 262144).is_none());
    }
    #[test]
    fn test_failed_trial_uses_previous_then_recovery() {
        let mut state = BootRecord::decode(&record(), 262144).unwrap();
        state.pending_copy = 1;
        assert_eq!(state.next_attempt(), Some(1));
        let mut rebooted = BootRecord::decode(&state.encode(), 262144).unwrap();
        assert_eq!(rebooted.next_attempt(), Some(0));
        let mut rebooted = BootRecord::decode(&rebooted.encode(), 262144).unwrap();
        assert_eq!(rebooted.next_attempt(), None);
        assert!(rebooted.confirm(0));
        assert_eq!(rebooted.active_copy, 0);
        assert_eq!(rebooted.next_attempt(), Some(0));
    }
    #[test]
    fn test_partition_rejects_overlap_and_card_overrun() {
        let mut mbr = [0; 512];
        mbr[510..].copy_from_slice(&[0x55, 0xaa]);
        mbr[450] = PARTITION_TYPE;
        mbr[454..458].copy_from_slice(&2048u32.to_le_bytes());
        mbr[458..462].copy_from_slice(&262144u32.to_le_bytes());
        assert_eq!(find_partition(&mbr, 1000000), Some((2048, 262144)));
        assert!(find_partition(&mbr, 10000).is_none());
        mbr[466] = 0x83;
        mbr[470..474].copy_from_slice(&4096u32.to_le_bytes());
        mbr[474..478].copy_from_slice(&2048u32.to_le_bytes());
        assert!(find_partition(&mbr, 1000000).is_none());
    }
}
