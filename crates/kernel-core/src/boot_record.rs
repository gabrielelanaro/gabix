//! The SD loader's single, checksummed boot decision and kernel extents.
//! Extents are relative to the kernel partition, so relocation changes the MBR.
pub const MAGIC: &[u8; 8] = b"GABIXBT1";
pub const PARTITION_TYPE: u8 = 0xda;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelCopy {
    pub offset_sectors: u64,
    pub capacity_sectors: u64,
    pub image_bytes: u32,
    pub image_crc32: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootRecord {
    pub active_copy: usize,
    pub copies: [KernelCopy; 2],
}

impl BootRecord {
    pub fn decode(bytes: &[u8; 512], partition_sectors: u64) -> Option<Self> {
        if &bytes[..8] != MAGIC
            || u32_at(bytes, 8) != 1
            || crc32(&bytes[..508]) != u32_at(bytes, 508)
        {
            return None;
        }
        let active_copy = u32_at(bytes, 12) as usize;
        if active_copy > 1 {
            return None;
        }
        let copies = [copy_at(bytes, 16), copy_at(bytes, 40)];
        for copy in copies {
            if copy.offset_sectors == 0
                || copy.capacity_sectors == 0
                || copy.offset_sectors.checked_add(copy.capacity_sectors)? > partition_sectors
                || copy.image_bytes as u64 > copy.capacity_sectors.checked_mul(512)?
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
        Some(Self {
            active_copy,
            copies,
        })
    }
}

/// Return the unique kernel partition only when all occupied extents fit and
/// none overlap it. The caller reads the boot record from its first sector.
pub fn find_partition(mbr: &[u8; 512], card_sectors: u64) -> Option<(u64, u64)> {
    if mbr[510..] != [0x55, 0xaa] {
        return None;
    }
    let mut found = None;
    for entry in mbr[446..510].chunks_exact(16) {
        if entry[4] == PARTITION_TYPE {
            if found.is_some() {
                return None;
            }
            found = Some((u32_at(entry, 8) as u64, u32_at(entry, 12) as u64));
        }
    }
    let (start, count) = found?;
    if start == 0 || count == 0 || start.checked_add(count)? > card_sectors {
        return None;
    }
    for entry in mbr[446..510].chunks_exact(16) {
        if entry[4] == 0 {
            continue;
        }
        let other_start = u32_at(entry, 8) as u64;
        let other_count = u32_at(entry, 12) as u64;
        let other_end = other_start.checked_add(other_count)?;
        if other_start == 0 || other_count == 0 || other_end > card_sectors {
            return None;
        }
        if entry[4] != PARTITION_TYPE && start < other_end && other_start < start + count {
            return None;
        }
    }
    Some((start, count))
}

pub fn update_crc(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
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
        offset_sectors: u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()),
        capacity_sectors: u64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().unwrap()),
        image_bytes: u32_at(bytes, offset + 16),
        image_crc32: u32_at(bytes, offset + 20),
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
