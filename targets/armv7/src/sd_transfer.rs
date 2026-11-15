//! Bounded Ethernet uploads into staging storage or the inactive kernel copy.
//! UDP retries acknowledge only sectors already written and read back.
use crate::raspi2b::sdhci::SdCard;
use gabix_kernel::device::WritableBlockDevice;

pub const PORT: u16 = 5556;
const HEADER_SECTOR: u64 = 8192;
const DATA_SECTOR: u64 = HEADER_SECTOR + 1;
const END_SECTOR: u64 = 12288;
const MAX_BYTES: u32 = ((END_SECTOR - DATA_SECTOR) * 512) as u32;
const DISK_MAGIC: &[u8; 8] = b"GABIXNET";
const WIRE_MAGIC: &[u8; 4] = b"GBSD";
const HEADER_LEN: usize = 20;
const BEGIN: u32 = 1;
const DATA: u32 = 2;
const FINISH: u32 = 3;
const READ: u32 = 4;
const OPEN: u32 = 5;
const INSTALL: u32 = 6;
const READ_SECTOR: u32 = 7;
const CARD_INFO: u32 = 8;
const BEGIN_KERNEL: u32 = 9;
const OK: u32 = 0;
const INVALID: u32 = 1;
const ORDER: u32 = 2;
const IO_ERROR: u32 = 3;
const CHECKSUM: u32 = 4;

pub struct Receiver {
    card: SdCard,
    session: u32,
    length: u32,
    expected_crc: u32,
    next: u32,
    verified: bool,
    failed: bool,
    peer: Option<([u8; 4], u16)>,
    data_sector: u64,
    kernel_target: Option<(u64, usize)>,
}

impl Receiver {
    pub unsafe fn initialize() -> Option<Self> {
        let mut card = SdCard::initialize()?;
        let mbr = card.try_read_sector(0)?;
        // This is the registered recovery image, with its first partition at
        // sector 16384. The source image's entire preceding gap was zero-filled.
        let expected: [u8; 32] = [
            0, 0, 1, 128, 12, 3, 224, 255, 0, 64, 0, 0, 0, 0, 16, 0, 0, 3, 224, 255, 131, 3, 224,
            255, 0, 64, 16, 0, 0, 64, 58, 0,
        ];
        if mbr[440..444] != [0x8d, 0x2d, 0x1b, 0x41]
            || mbr[446..478] != expected
            || (mbr[478..510] != [0; 32]
                && gabix_kernel::boot_record::find_partition(&mbr, card.sector_count())
                    != Some((4882432, 262144)))
            || mbr[510..] != [0x55, 0xaa]
        {
            return None;
        }
        let header = card.try_read_sector(HEADER_SECTOR)?;
        let owned = header[..8] == *DISK_MAGIC && word(&header, 20) == crc32(&header[..20]);
        if !owned {
            // First use requires an empty region, not merely an empty header.
            for sector in HEADER_SECTOR..END_SECTOR {
                if sector % 64 == 0 {
                    crate::raspi2b::reset::arm_deadman(15);
                }
                if card.try_read_sector(sector)? != [0; 512] {
                    return None;
                }
            }
        }
        let length = if owned { word(&header, 8) } else { 0 };
        if owned && (length == 0 || length > MAX_BYTES || word(&header, 16) > 1) {
            return None;
        }
        let verified = owned && word(&header, 16) == 1;
        Some(Self {
            card,
            session: 0,
            length,
            expected_crc: if owned { word(&header, 12) } else { 0 },
            next: if verified { length } else { 0 },
            verified,
            failed: false,
            peer: None,
            data_sector: DATA_SECTOR,
            kernel_target: None,
        })
    }

    pub fn handle(
        &mut self,
        peer: ([u8; 4], u16),
        packet: &[u8],
        reply: &mut [u8; 532],
    ) -> Option<usize> {
        if packet.len() < HEADER_LEN || &packet[..4] != WIRE_MAGIC {
            return None;
        }
        let operation = word(packet, 4);
        let session = word(packet, 8);
        let argument = word(packet, 12);
        let value = word(packet, 16);
        let data = &packet[HEADER_LEN..];
        let mut status = INVALID;
        let mut reply_data_len = 0;
        if self.failed {
            status = IO_ERROR;
        } else if operation == CARD_INFO && data.is_empty() {
            reply[20..28].copy_from_slice(&self.card.sector_count().to_le_bytes());
            reply_data_len = 8;
            status = OK;
        } else if operation == READ_SECTOR
            && data.is_empty()
            && crate::sd_install::readable(argument as u64)
        {
            if let Some(sector) = unsafe { self.card.try_read_sector(argument as u64) } {
                reply[20..532].copy_from_slice(&sector);
                reply_data_len = 512;
                status = OK;
            } else {
                status = IO_ERROR;
            }
        } else if operation == OPEN && data.is_empty() && session != 0 && self.verified {
            self.peer = Some(peer);
            self.session = session;
            reply[20..24].copy_from_slice(&self.length.to_le_bytes());
            reply[24..28].copy_from_slice(&self.expected_crc.to_le_bytes());
            reply_data_len = 8;
            status = OK;
        } else if matches!(operation, BEGIN | BEGIN_KERNEL)
            && data.is_empty()
            && session != 0
            && argument > 0
            && argument
                <= if operation == BEGIN_KERNEL {
                    gabix_kernel::loader_protocol::MAX_KERNEL_SIZE
                } else {
                    MAX_BYTES
                }
        {
            if self.peer == Some(peer) && self.session == session {
                status = if self.length == argument
                    && self.expected_crc == value
                    && self.kernel_target.is_some() == (operation == BEGIN_KERNEL)
                {
                    OK
                } else {
                    INVALID
                };
            } else {
                let target = if operation == BEGIN_KERNEL {
                    self.inactive_kernel(argument)
                } else {
                    Some((DATA_SECTOR, None))
                };
                if let Some((data_sector, kernel_target)) = target {
                    self.data_sector = data_sector;
                    self.kernel_target = kernel_target;
                    self.peer = Some(peer);
                    self.session = session;
                    self.length = argument;
                    self.expected_crc = value;
                    self.next = 0;
                    self.verified = false;
                    status = if self.save_header(false) {
                        OK
                    } else {
                        IO_ERROR
                    };
                }
            }
        } else if self.peer == Some(peer) && self.session == session {
            match operation {
                DATA if argument % 512 == 0 && argument < self.length => {
                    let expected_len = (self.length - argument).min(512) as usize;
                    if data.len() == expected_len && crc32(data) == value {
                        if argument > self.next {
                            status = ORDER;
                        } else if argument < self.next {
                            // A lost ACK must not cause another flash write.
                            status = match unsafe {
                                self.card
                                    .try_read_sector(self.data_sector + argument as u64 / 512)
                            } {
                                Some(stored) if stored[..data.len()] == *data => OK,
                                Some(_) => CHECKSUM,
                                None => IO_ERROR,
                            };
                        } else if !self.verified {
                            let mut sector = [0; 512];
                            sector[..data.len()].copy_from_slice(data);
                            let lba = self.data_sector + argument as u64 / 512;
                            status = if self.card.write_sector(lba, &sector).is_ok()
                                && unsafe { self.card.try_read_sector(lba) } == Some(sector)
                            {
                                self.next += data.len() as u32;
                                OK
                            } else {
                                IO_ERROR
                            };
                        }
                    } else {
                        status = CHECKSUM;
                    }
                }
                INSTALL if data.is_empty() && self.verified && self.kernel_target.is_none() => {
                    status = if crate::sd_install::apply(&mut self.card, DATA_SECTOR, self.length)
                        .is_some()
                    {
                        OK
                    } else {
                        INVALID
                    };
                }
                FINISH if data.is_empty() && self.next == self.length => {
                    let mut crc = !0;
                    status = OK;
                    for offset in (0..self.length).step_by(512) {
                        if let Some(sector) = unsafe {
                            self.card
                                .try_read_sector(self.data_sector + offset as u64 / 512)
                        } {
                            crc = update_crc(
                                crc,
                                &sector[..(self.length - offset).min(512) as usize],
                            );
                        } else {
                            status = IO_ERROR;
                            break;
                        }
                    }
                    if status == OK {
                        if !crc != self.expected_crc {
                            status = CHECKSUM;
                        } else if !self.verified && !self.save_header(true) {
                            status = IO_ERROR;
                        } else {
                            self.verified = true;
                        }
                    }
                }
                READ if data.is_empty()
                    && self.verified
                    && argument % 512 == 0
                    && argument < self.length =>
                {
                    status = match unsafe {
                        self.card
                            .try_read_sector(self.data_sector + argument as u64 / 512)
                    } {
                        Some(sector) => {
                            reply_data_len = (self.length - argument).min(512) as usize;
                            reply[HEADER_LEN..HEADER_LEN + reply_data_len]
                                .copy_from_slice(&sector[..reply_data_len]);
                            OK
                        }
                        None => IO_ERROR,
                    };
                }
                _ => {}
            }
        }
        if status == IO_ERROR {
            self.failed = true;
        }
        reply[..4].copy_from_slice(WIRE_MAGIC);
        for (offset, value) in [
            (4, operation | 0x80000000),
            (8, session),
            (12, argument),
            (16, status),
        ] {
            reply[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        Some(HEADER_LEN + reply_data_len)
    }

    pub fn confirm_boot(&mut self) -> bool {
        crate::boot_trial::confirm(&mut self.card)
    }

    fn inactive_kernel(&mut self, length: u32) -> Option<(u64, Option<(u64, usize)>)> {
        use gabix_kernel::boot_record::{find_partition, BootRecord};
        unsafe {
            let mbr = self.card.try_read_sector(0)?;
            let (start, count) = find_partition(&mbr, self.card.sector_count())?;
            let record = BootRecord::decode(&self.card.try_read_sector(start)?, count)?;
            let index = 1 - record.active_copy;
            let copy = record.copies[index];
            if length as u64 > copy.capacity_sectors * 512 {
                return None;
            }
            Some((start + copy.offset_sectors, Some((start, index))))
        }
    }

    fn save_header(&mut self, complete: bool) -> bool {
        if let Some((start, index)) = self.kernel_target {
            use gabix_kernel::boot_record::{find_partition, BootRecord, NO_COPY};
            return (|| unsafe {
                let mbr = self.card.try_read_sector(0)?;
                let (actual, count) = find_partition(&mbr, self.card.sector_count())?;
                if actual != start {
                    return None;
                }
                let mut record = BootRecord::decode(&self.card.try_read_sector(start)?, count)?;
                if record.active_copy == index {
                    return None;
                }
                record.copies[index].image_bytes = if complete { self.length } else { 0 };
                record.copies[index].image_crc32 = if complete { self.expected_crc } else { 0 };
                record.pending_copy = if complete { index as u32 } else { NO_COPY };
                record.attempted_mask = 0;
                record.running_copy = NO_COPY;
                let bytes = record.encode();
                self.card.write_sector(start, &bytes).ok()?;
                (self.card.try_read_sector(start)? == bytes).then_some(())
            })()
            .is_some();
        }
        let mut header = [0; 512];
        header[..8].copy_from_slice(DISK_MAGIC);
        header[8..12].copy_from_slice(&self.length.to_le_bytes());
        header[12..16].copy_from_slice(&self.expected_crc.to_le_bytes());
        header[16..20].copy_from_slice(&(complete as u32).to_le_bytes());
        let checksum = crc32(&header[..20]);
        header[20..24].copy_from_slice(&checksum.to_le_bytes());
        self.card.write_sector(HEADER_SECTOR, &header).is_ok()
            && unsafe { self.card.try_read_sector(HEADER_SECTOR) } == Some(header)
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn update_crc(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    crc
}
fn crc32(bytes: &[u8]) -> u32 {
    !update_crc(!0, bytes)
}
