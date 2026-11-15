//! Bounded development installer. Packages contain expected-old and new sectors.
//! Preflight checks every record before any write; boot-file writes come last.
use crate::raspi2b::sdhci::SdCard;
use gabix_kernel::boot_record::{crc32, BootRecord};
use gabix_kernel::device::WritableBlockDevice;

pub const KERNEL_START: u64 = 4_882_432;
pub const KERNEL_SECTORS: u64 = 262_144;
const FAT_START: u64 = 16_384;
const FAT_SECTORS: u64 = 1_048_576;
const MAX_LOADER_SECTORS: usize = 512; // 256 KiB, below the loader's RAM handoff.

pub fn readable(lba: u64) -> bool {
    lba == 0
        || (FAT_START..FAT_START + FAT_SECTORS).contains(&lba)
        || (KERNEL_START..KERNEL_START + KERNEL_SECTORS).contains(&lba)
}

pub fn apply(card: &mut SdCard, package_start: u64, bytes: u32) -> Option<u32> {
    let header = read(card, package_start)?;
    if &header[..8] != b"GBXINST1" || word(&header, 12) != 1 {
        return None;
    }
    let count = word(&header, 8);
    if count == 0 || count > 1000 || bytes as u64 != (1 + 3 * count as u64) * 512 {
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
        if phase == 0 {
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
    if (KERNEL_START + 2048..KERNEL_START + 133120).contains(&lba) {
        return Some(0);
    }
    if lba == KERNEL_START {
        let record = BootRecord::decode(new, KERNEL_SECTORS)?;
        if record.copies[0].offset_sectors != 2048
            || record.copies[1].offset_sectors != 67584
            || record
                .copies
                .iter()
                .any(|copy| copy.capacity_sectors != 65536)
        {
            return None;
        }
        return Some(1);
    }
    if lba == 0 {
        let mut expected = *mbr;
        expected[478..494].fill(0);
        expected[482] = 0xda;
        expected[486..490].copy_from_slice(&(KERNEL_START as u32).to_le_bytes());
        expected[490..494].copy_from_slice(&(KERNEL_SECTORS as u32).to_le_bytes());
        return (*new == expected).then_some(2);
    }
    if boot.sectors[..boot.count].contains(&lba) {
        return Some(3);
    }
    if lba == boot.directory_sector {
        let size = word(new, boot.entry_offset + 28) as usize;
        let mut expected = *old;
        expected[boot.entry_offset + 28..boot.entry_offset + 32]
            .copy_from_slice(&(size as u32).to_le_bytes());
        if size == 0 || size > boot.count * 512 || *new != expected {
            return None;
        }
        return Some(4);
    }
    None
}

fn package_record(
    card: &mut SdCard,
    start: u64,
    index: u32,
) -> Option<(u64, [u8; 512], [u8; 512])> {
    let base = start + 1 + index as u64 * 3;
    let descriptor = read(card, base)?;
    let lba = u64::from_le_bytes(descriptor[..8].try_into().unwrap());
    let old = read(card, base + 1)?;
    let new = read(card, base + 2)?;
    if crc32(&old) != word(&descriptor, 8) || crc32(&new) != word(&descriptor, 12) {
        return None;
    }
    Some((lba, old, new))
}
fn known_mbr(mbr: &[u8; 512]) -> bool {
    let expected: [u8; 32] = [
        0, 0, 1, 128, 12, 3, 224, 255, 0, 64, 0, 0, 0, 0, 16, 0, 0, 3, 224, 255, 131, 3, 224, 255,
        0, 64, 16, 0, 0, 64, 58, 0,
    ];
    mbr[440..444] == [0x8d, 0x2d, 0x1b, 0x41]
        && mbr[446..478] == expected
        && mbr[494..510] == [0; 16]
        && mbr[510..] == [0x55, 0xaa]
        && (mbr[478..494] == [0; 16]
            || (mbr[482] == 0xda
                && word(mbr, 486) as u64 == KERNEL_START
                && word(mbr, 490) as u64 == KERNEL_SECTORS))
}
fn read(card: &mut SdCard, lba: u64) -> Option<[u8; 512]> {
    unsafe { card.try_read_sector(lba) }
}
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

struct BootFile {
    directory_sector: u64,
    entry_offset: usize,
    sectors: [u64; MAX_LOADER_SECTORS],
    count: usize,
}
impl BootFile {
    fn read(card: &mut SdCard) -> Option<Self> {
        let boot = read(card, FAT_START)?;
        if boot[11..13] != 512u16.to_le_bytes() || &boot[82..90] != b"FAT32   " {
            return None;
        }
        let cluster_sectors = boot[13] as u64;
        let reserved = u16::from_le_bytes(boot[14..16].try_into().unwrap()) as u64;
        let fat_size = word(&boot, 36) as u64;
        if cluster_sectors == 0 || !cluster_sectors.is_power_of_two() || fat_size == 0 {
            return None;
        }
        let fat_start = FAT_START + reserved;
        let data_start = fat_start + boot[16] as u64 * fat_size;
        let cluster_lba = |cluster: u32| -> Option<u64> {
            if cluster < 2 {
                return None;
            }
            let start = data_start + (cluster as u64 - 2) * cluster_sectors;
            (start + cluster_sectors <= FAT_START + FAT_SECTORS).then_some(start)
        };
        let mut root = word(&boot, 44);
        let mut found = None;
        'directory: for _ in 0..64 {
            let start = cluster_lba(root)?;
            for sector in start..start + cluster_sectors {
                let entries = read(card, sector)?;
                for offset in (0..512).step_by(32) {
                    let entry = &entries[offset..offset + 32];
                    if entry[0] == 0 {
                        return None;
                    }
                    if &entry[..11] == b"KERNEL7 IMG" && entry[11] & 0x18 == 0 {
                        let cluster =
                            ((u16::from_le_bytes(entry[20..22].try_into().unwrap()) as u32) << 16)
                                | u16::from_le_bytes(entry[26..28].try_into().unwrap()) as u32;
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
            if next >= 0x0ffffff8 {
                break;
            }
            cluster = next;
        }
        Some(result)
    }
}
fn next_cluster(card: &mut SdCard, fat_start: u64, fat_size: u64, cluster: u32) -> Option<u32> {
    let offset = cluster as u64 * 4;
    if offset / 512 >= fat_size {
        return None;
    }
    Some(
        word(
            &read(card, fat_start + offset / 512)?,
            (offset % 512) as usize,
        ) & 0x0fffffff,
    )
}
