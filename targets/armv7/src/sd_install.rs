//! Bounded development installer. Packages contain expected-old and new sectors.
//! Preflight checks every record before any write; boot-file writes come last.
use crate::raspi2b::sdhci::SdCard;
use gabix_kernel::boot_record::{crc32, BootRecord, PARTITION_TYPE};
use gabix_kernel::device::WritableBlockDevice;

pub const KERNEL_START: u64 = 4_882_432;
pub const KERNEL_SECTORS: u64 = 262_144;
const FAT_START: u64 = 16_384;
const FAT_SECTORS: u64 = 1_048_576;
const SECTOR_BYTES: u64 = 512;
const COPY_A_OFFSET_SECTORS: u64 = 2048;
const COPY_B_OFFSET_SECTORS: u64 = 67584;
const COPY_CAPACITY_SECTORS: u64 = 65536;
const COPY_AREA_END_SECTORS: u64 = COPY_B_OFFSET_SECTORS + COPY_CAPACITY_SECTORS;
const MAX_PACKAGE_RECORDS: u32 = 1000;
// Ascending phases enforce publication order: payloads before boot selection.
const PHASE_KERNEL_DATA: u8 = 0;
const PHASE_BOOT_RECORD: u8 = 1;
const PHASE_PARTITION_TABLE: u8 = 2;
const PHASE_LOADER_DATA: u8 = 3;
const PHASE_LOADER_FILE_SIZE: u8 = 4;

// Same little-endian format emitted by scripts/pi2-install.py::write_package.
mod package_field {
    pub const MAGIC: &[u8; 8] = b"GBXINST1";
    pub const COUNT_OFFSET: usize = 8;
    pub const VERSION_OFFSET: usize = 12;
    pub const VERSION: u32 = 1;
    pub const HEADER_SECTORS: u64 = 1;
    pub const RECORD_SECTORS: u64 = 3; // descriptor, expected-old bytes, new bytes
    pub const LBA_BYTES: core::ops::Range<usize> = 0..8;
    pub const OLD_CRC_OFFSET: usize = 8;
    pub const NEW_CRC_OFFSET: usize = 12;
}
const MAX_LOADER_SECTORS: usize = 512; // 256 KiB, below the loader's RAM handoff.

/// Installer inspection allowlist: MBR, fixed FAT partition and kernel region.
/// This does not check card capacity; the subsequent device read can still fail.
pub fn readable(lba: u64) -> bool {
    lba == 0
        || (FAT_START..FAT_START + FAT_SECTORS).contains(&lba)
        || (KERNEL_START..KERNEL_START + KERNEL_SECTORS).contains(&lba)
}

/// Validate the whole package against the current card, then write in order.
/// Callers must supply stable staged bytes, a valid non-overlapping staging
/// extent, and serialize all card mutation. The Ethernet receiver supplies its
/// fixed staging area; this helper does not independently bound package_start.
/// Failure may leave an applied prefix; sectors already equal to new are accepted
/// on retry. This is not a transactional or power-loss-atomic operation.
pub fn apply(card: &mut SdCard, package_start: u64, bytes: u32) -> Option<u32> {
    let header = read(card, package_start)?;
    if &header[..package_field::MAGIC.len()] != package_field::MAGIC
        || word(&header, package_field::VERSION_OFFSET) != package_field::VERSION
    {
        return None;
    }
    let count = word(&header, package_field::COUNT_OFFSET);
    if count == 0
        || count > MAX_PACKAGE_RECORDS
        || bytes as u64
            != (package_field::HEADER_SECTORS + package_field::RECORD_SECTORS * count as u64)
                * SECTOR_BYTES
    {
        return None;
    }
    if card.sector_count() < KERNEL_START + KERNEL_SECTORS {
        return None;
    }
    let mbr = read(card, 0)?;
    if !known_mbr(&mbr) {
        return None;
    }
    let boot = BootFile::read(card)?;
    let existing = read(card, KERNEL_START)?;
    // An undecodable record is treated as a fresh install. Active-copy protection
    // below applies only when an existing record decodes successfully.
    let active = BootRecord::decode(&existing, KERNEL_SECTORS);
    let mut last = None;
    for index in 0..count {
        let (lba, old, new) = package_record(card, package_start, index)?;
        let phase = allowed(&boot, &mbr, lba, &old, &new)?;
        if let Some((previous_phase, previous_lba)) = last {
            if phase < previous_phase || (phase == previous_phase && lba <= previous_lba) {
                return None;
            }
        }
        last = Some((phase, lba));
        let actual = read(card, lba)?;
        if actual != old && actual != new {
            return None;
        }
        if phase == PHASE_KERNEL_DATA {
            if let Some(record) = active {
                let copy = record.copies[record.active_copy];
                let start = KERNEL_START + copy.offset_sectors;
                if (start..start + copy.capacity_sectors).contains(&lba) && actual != new {
                    return None; // Updates must preserve the last confirmed kernel.
                }
            }
        }
    }
    // Every address, expected sector, and ordering constraint passed preflight.
    // Once a write fails, return immediately; the sender can inspect and retry.
    for index in 0..count {
        let (lba, _, new) = package_record(card, package_start, index)?;
        if read(card, lba)? != new {
            card.write_sector(lba, &new).ok()?;
            if read(card, lba)? != new {
                return None;
            }
        }
    }
    Some(count)
}

fn allowed(
    boot: &BootFile,
    mbr: &[u8; 512],
    lba: u64,
    old: &[u8; 512],
    new: &[u8; 512],
) -> Option<u8> {
    if (KERNEL_START + COPY_A_OFFSET_SECTORS..KERNEL_START + COPY_AREA_END_SECTORS).contains(&lba) {
        return Some(PHASE_KERNEL_DATA);
    }
    if lba == KERNEL_START {
        let record = BootRecord::decode(new, KERNEL_SECTORS)?;
        if record.copies[0].offset_sectors != COPY_A_OFFSET_SECTORS
            || record.copies[1].offset_sectors != COPY_B_OFFSET_SECTORS
            || record
                .copies
                .iter()
                .any(|copy| copy.capacity_sectors != COPY_CAPACITY_SECTORS)
        {
            return None;
        }
        return Some(PHASE_BOOT_RECORD);
    }
    if lba == 0 {
        // Preserve every other byte and replace only the third primary entry.
        let mut expected = *mbr;
        expected[mbr_field::KERNEL_ENTRY].fill(0);
        expected[mbr_field::KERNEL_TYPE] = PARTITION_TYPE;
        expected[mbr_field::KERNEL_START].copy_from_slice(&(KERNEL_START as u32).to_le_bytes());
        expected[mbr_field::KERNEL_SIZE].copy_from_slice(&(KERNEL_SECTORS as u32).to_le_bytes());
        return (*new == expected).then_some(PHASE_PARTITION_TABLE);
    }
    if boot.sectors[..boot.count].contains(&lba) {
        return Some(PHASE_LOADER_DATA);
    }
    if lba == boot.directory_sector {
        let size = word(new, boot.entry_offset + fat_field::FILE_SIZE) as usize;
        let mut expected = *old;
        expected[boot.entry_offset + fat_field::FILE_SIZE
            ..boot.entry_offset + fat_field::DIRECTORY_ENTRY_BYTES]
            .copy_from_slice(&(size as u32).to_le_bytes());
        if size == 0 || size > boot.count * 512 || *new != expected {
            return None;
        }
        return Some(PHASE_LOADER_FILE_SIZE);
    }
    None
}

fn package_record(
    card: &mut SdCard,
    start: u64,
    index: u32,
) -> Option<(u64, [u8; 512], [u8; 512])> {
    let base = start + package_field::HEADER_SECTORS + index as u64 * package_field::RECORD_SECTORS;
    let descriptor = read(card, base)?;
    let lba = u64::from_le_bytes(descriptor[package_field::LBA_BYTES].try_into().unwrap());
    let old = read(card, base + 1)?;
    let new = read(card, base + 2)?;
    if crc32(&old) != word(&descriptor, package_field::OLD_CRC_OFFSET)
        || crc32(&new) != word(&descriptor, package_field::NEW_CRC_OFFSET)
    {
        return None;
    }
    Some((lba, old, new))
}
// Primary-entry byte ranges in sector 0, matching the host installer's layout.
mod mbr_field {
    pub const DISK_SIGNATURE: core::ops::Range<usize> = 440..444;
    pub const EXISTING_PARTITIONS: core::ops::Range<usize> = 446..478;
    pub const KERNEL_ENTRY: core::ops::Range<usize> = 478..494;
    pub const KERNEL_TYPE: usize = 482;
    pub const KERNEL_START: core::ops::Range<usize> = 486..490;
    pub const KERNEL_SIZE: core::ops::Range<usize> = 490..494;
    pub const UNUSED_ENTRY: core::ops::Range<usize> = 494..510;
    pub const SIGNATURE: core::ops::Range<usize> = 510..512;
}

// Exact identity/layout fingerprint of the development card, including CHS bytes.
// Boot code and bytes outside these fields are not part of this identity check.
fn known_mbr(mbr: &[u8; 512]) -> bool {
    const EXPECTED_FIRST_TWO_PARTITIONS: [u8; 32] = [
        0, 0, 1, 128, 12, 3, 224, 255, 0, 64, 0, 0, 0, 0, 16, 0, 0, 3, 224, 255, 131, 3, 224, 255,
        0, 64, 16, 0, 0, 64, 58, 0,
    ];
    mbr[mbr_field::DISK_SIGNATURE] == [0x8d, 0x2d, 0x1b, 0x41]
        && mbr[mbr_field::EXISTING_PARTITIONS] == EXPECTED_FIRST_TWO_PARTITIONS
        && mbr[mbr_field::UNUSED_ENTRY] == [0; 16]
        && mbr[mbr_field::SIGNATURE] == [0x55, 0xaa]
        && (mbr[mbr_field::KERNEL_ENTRY] == [0; 16]
            || (mbr[mbr_field::KERNEL_TYPE] == PARTITION_TYPE
                && word(mbr, mbr_field::KERNEL_START.start) as u64 == KERNEL_START
                && word(mbr, mbr_field::KERNEL_SIZE.start) as u64 == KERNEL_SECTORS))
}
fn read(card: &mut SdCard, lba: u64) -> Option<[u8; 512]> {
    unsafe { card.try_read_sector(lba) }
}
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

// FAT32 fields used by this fixed-card installer (Microsoft FAT specification).
mod fat_field {
    pub const BYTES_PER_SECTOR: core::ops::Range<usize> = 11..13;
    pub const SECTORS_PER_CLUSTER: usize = 13;
    pub const RESERVED_SECTORS: core::ops::Range<usize> = 14..16;
    pub const FAT_COUNT: usize = 16;
    pub const FAT_SECTORS: usize = 36;
    pub const ROOT_CLUSTER: usize = 44;
    pub const TYPE_LABEL: core::ops::Range<usize> = 82..90;
    pub const DIRECTORY_ENTRY_BYTES: usize = 32;
    pub const SHORT_NAME: core::ops::Range<usize> = 0..11;
    pub const ATTRIBUTES: usize = 11;
    pub const VOLUME_OR_DIRECTORY: u8 = 0x08 | 0x10;
    pub const CLUSTER_HIGH: core::ops::Range<usize> = 20..22;
    pub const CLUSTER_LOW: core::ops::Range<usize> = 26..28;
    pub const FILE_SIZE: usize = 28;
    pub const FIRST_DATA_CLUSTER: u32 = 2;
    pub const ENTRY_BYTES: u64 = 4;
    pub const CLUSTER_MASK: u32 = 0x0fffffff;
    pub const END_OF_CHAIN: u32 = 0x0ffffff8;
}
const MAX_ROOT_DIRECTORY_CLUSTERS: usize = 64;

// The sector list describes existing allocation, including space beyond file size.
struct BootFile {
    directory_sector: u64,
    entry_offset: usize,
    sectors: [u64; MAX_LOADER_SECTORS],
    count: usize,
}
impl BootFile {
    // Locate the fixed 8.3 name using the first FAT. This checks selected fields;
    // it does not derive FAT type from cluster count or validate the full BPB.
    fn read(card: &mut SdCard) -> Option<Self> {
        let boot = read(card, FAT_START)?;
        if boot[fat_field::BYTES_PER_SECTOR] != 512u16.to_le_bytes()
            || &boot[fat_field::TYPE_LABEL] != b"FAT32   "
        {
            return None;
        }
        let cluster_sectors = boot[fat_field::SECTORS_PER_CLUSTER] as u64;
        let reserved =
            u16::from_le_bytes(boot[fat_field::RESERVED_SECTORS].try_into().unwrap()) as u64;
        let fat_size = word(&boot, fat_field::FAT_SECTORS) as u64;
        if cluster_sectors == 0 || !cluster_sectors.is_power_of_two() || fat_size == 0 {
            return None;
        }
        let fat_start = FAT_START + reserved;
        let data_start = fat_start + boot[fat_field::FAT_COUNT] as u64 * fat_size;
        let cluster_lba = |cluster: u32| -> Option<u64> {
            if cluster < fat_field::FIRST_DATA_CLUSTER {
                return None;
            }
            let start = data_start
                + (cluster as u64 - fat_field::FIRST_DATA_CLUSTER as u64) * cluster_sectors;
            (start + cluster_sectors <= FAT_START + FAT_SECTORS).then_some(start)
        };
        let mut root = word(&boot, fat_field::ROOT_CLUSTER);
        let mut found = None;
        'directory: for _ in 0..MAX_ROOT_DIRECTORY_CLUSTERS {
            let start = cluster_lba(root)?;
            for sector in start..start + cluster_sectors {
                let entries = read(card, sector)?;
                for offset in (0..SECTOR_BYTES as usize).step_by(fat_field::DIRECTORY_ENTRY_BYTES) {
                    let entry = &entries[offset..offset + fat_field::DIRECTORY_ENTRY_BYTES];
                    if entry[0] == 0 {
                        return None;
                    }
                    if &entry[fat_field::SHORT_NAME] == b"KERNEL7 IMG"
                        && entry[fat_field::ATTRIBUTES] & fat_field::VOLUME_OR_DIRECTORY == 0
                    {
                        let cluster = ((u16::from_le_bytes(
                            entry[fat_field::CLUSTER_HIGH].try_into().unwrap(),
                        ) as u32)
                            << 16)
                            | u16::from_le_bytes(entry[fat_field::CLUSTER_LOW].try_into().unwrap())
                                as u32;
                        found = Some((sector, offset, cluster));
                        break 'directory;
                    }
                }
            }
            root = next_cluster(card, fat_start, fat_size, root)?;
        }
        let (directory_sector, entry_offset, mut cluster) = found?;
        let mut result = Self {
            directory_sector,
            entry_offset,
            sectors: [0; MAX_LOADER_SECTORS],
            count: 0,
        };
        // Keep at most the loader-size cap, even if the chain continues. Repeated
        // data sectors and the selected directory sector are rejected; other
        // files' allocations are not checked for cross-links.
        while result.count < MAX_LOADER_SECTORS {
            let start = cluster_lba(cluster)?;
            for sector in start..start + cluster_sectors {
                if result.count == MAX_LOADER_SECTORS {
                    break;
                }
                if sector == directory_sector || result.sectors[..result.count].contains(&sector) {
                    return None;
                }
                result.sectors[result.count] = sector;
                result.count += 1;
            }
            let next = next_cluster(card, fat_start, fat_size, cluster)?;
            if next >= fat_field::END_OF_CHAIN {
                break;
            }
            cluster = next;
        }
        Some(result)
    }
}
// Bounds the entry within the first FAT and returns its low 28 bits. Free,
// reserved and bad-cluster values are passed back for the caller to encounter.
fn next_cluster(card: &mut SdCard, fat_start: u64, fat_size: u64, cluster: u32) -> Option<u32> {
    let offset = cluster as u64 * fat_field::ENTRY_BYTES;
    if offset / SECTOR_BYTES >= fat_size {
        return None;
    }
    Some(
        word(
            &read(card, fat_start + offset / SECTOR_BYTES)?,
            (offset % SECTOR_BYTES) as usize,
        ) & fat_field::CLUSTER_MASK,
    )
}
