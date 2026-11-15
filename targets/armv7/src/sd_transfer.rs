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
// Little-endian UDP header; requests and replies share these positions.
mod wire_field {
    pub const OPERATION: usize = 4;
    pub const SESSION: usize = 8;
    pub const ARGUMENT: usize = 12;
    pub const VALUE_OR_STATUS: usize = 16;
}
const REPLY_OPERATION_BIT: u32 = 1 << 31;
// First 24 bytes of the staging header sector; CRC covers the first 20.
mod disk_field {
    pub const LENGTH: usize = 8;
    pub const IMAGE_CRC: usize = 12;
    pub const COMPLETE: usize = 16;
    pub const HEADER_CRC: usize = 20;
    pub const HEADER_BYTES: usize = 24;
}
const BEGIN: u32 = 1;
const DATA: u32 = 2;
const FINISH: u32 = 3;
const READ: u32 = 4;
const OPEN: u32 = 5;
const INSTALL: u32 = 6;
const READ_SECTOR: u32 = 7;
const CARD_INFO: u32 = 8;
const BEGIN_KERNEL: u32 = 9;
#[cfg(feature = "pi-managed")]
const AUTH_CHALLENGE: u32 = 10;
const STATUS: u32 = 11;
const READ_LOG: u32 = 12;
const REBOOT: u32 = 13;
const OK: u32 = 0;
const INVALID: u32 = 1;
const ORDER: u32 = 2;
const IO_ERROR: u32 = 3;
const CHECKSUM: u32 = 4;

pub struct Receiver {
    card: SdCard,
    pub journal: crate::diagnostics::Journal,
    #[cfg(feature = "pi-managed")]
    authentication: crate::management_auth::Authentication,
    status: [u8; 176],
    requests: u32,
    tx_failures: u32,
    last_request: u64,
    reboot_at: Option<u64>,
    last_error: (u32, u32),
    repeated_errors: u32,
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
        // A valid header identifies previously claimed staging storage. It does
        // not recheck the payload CRC here; verified is restored from its flag.
        let owned = header[..8] == *DISK_MAGIC && word(&header, disk_field::HEADER_CRC) == crc32(&header[..disk_field::HEADER_CRC]);
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
        let length = if owned { word(&header, disk_field::LENGTH) } else { 0 };
        if owned && (length == 0 || length > MAX_BYTES || word(&header, disk_field::COMPLETE) > 1) {
            return None;
        }
        let verified = owned && word(&header, disk_field::COMPLETE) == 1;
        let journal =
            crate::diagnostics::Journal::open(&mut card, crate::raspi2b::timer::uptime_millis())?;
        #[cfg(feature = "pi-managed")]
        if journal.boot == 0 {
            return None;
        }
        if let Some(bytes) = unsafe { card.try_read_sector(crate::sd_install::KERNEL_START) } {
            if let Some(record) = gabix_kernel::boot_record::BootRecord::decode(
                &bytes,
                crate::sd_install::KERNEL_SECTORS,
            ) {
                crate::diagnostics::event(format_args!(
                    "boot selection active={} pending={} attempted={} running={}",
                    record.active_copy,
                    record.pending_copy,
                    record.attempted_mask,
                    record.running_copy
                ));
            }
        }
        crate::diagnostics::event(format_args!(
            "reset_status_raw={:#x}",
            crate::raspi2b::reset::status()
        ));
        #[cfg(feature = "pi-managed")]
        let authentication = crate::management_auth::Authentication::new(journal.boot);
        Some(Self {
            card,
            journal,
            #[cfg(feature = "pi-managed")]
            authentication,
            status: [0; 176],
            requests: 0,
            tx_failures: 0,
            last_request: 0,
            reboot_at: None,
            last_error: (0, 0),
            repeated_errors: 0,
            session: 0,
            length,
            expected_crc: if owned { word(&header, disk_field::IMAGE_CRC) } else { 0 },
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
        reply: &mut [u8; 584],
    ) -> Option<usize> {
        #[cfg(feature = "pi-managed")]
        {
            // A public challenge exposes only the persistent boot number and
            // counter. Every diagnostic/read/write/reboot command requires HMAC.
            if packet.len() == 20 && &packet[..4] == WIRE_MAGIC && word(packet, wire_field::OPERATION) == 10 {
                reply[..20].copy_from_slice(packet);
                reply[4..8].copy_from_slice(&(AUTH_CHALLENGE | REPLY_OPERATION_BIT).to_le_bytes());
                reply[16..20].fill(0);
                reply[20..28].copy_from_slice(&self.authentication.boot.to_le_bytes());
                reply[28..36].copy_from_slice(&self.authentication.sequence.to_le_bytes());
                return Some(36);
            }
            match self.authentication.verify(packet) {
                crate::management_auth::Request::Rejected => return None,
                crate::management_auth::Request::Retry => {
                    return Some(self.authentication.reply(reply))
                }
                crate::management_auth::Request::Fresh(inner) => {
                    let mut response = [0; 532];
                    let length = self.handle_command(peer, inner, &mut response)?;
                    self.authentication.seal(&response[..length]);
                    return Some(self.authentication.reply(reply));
                }
            }
        }
        #[cfg(not(feature = "pi-managed"))]
        {
            let mut response = [0; 532];
            let length = self.handle_command(peer, packet, &mut response)?;
            reply[..length].copy_from_slice(&response[..length]);
            Some(length)
        }
    }

    fn handle_command(
        &mut self,
        peer: ([u8; 4], u16),
        packet: &[u8],
        reply: &mut [u8; 532],
    ) -> Option<usize> {
        if packet.len() < HEADER_LEN || &packet[..4] != WIRE_MAGIC {
            return None;
        }
        self.requests = self.requests.saturating_add(1);
        self.last_request = crate::raspi2b::timer::uptime_millis();
        let operation = word(packet, wire_field::OPERATION);
        let session = word(packet, wire_field::SESSION);
        let argument = word(packet, wire_field::ARGUMENT);
        let value = word(packet, wire_field::VALUE_OR_STATUS);
        let data = &packet[HEADER_LEN..];
        let mut status = INVALID;
        let mut reply_data_len = 0;
        if operation == STATUS && data.is_empty() {
            self.fill_storage_status();
            reply[20..196].copy_from_slice(&self.status);
            reply_data_len = 176;
            status = OK;
        } else if operation == READ_LOG && data.is_empty() {
            let sequence = (value as u64) << 32 | argument as u64;
            if let Some(bytes) = self.journal.read(&mut self.card, sequence) {
                reply[20..532].copy_from_slice(&bytes);
                reply_data_len = 512;
                status = OK;
            }
        } else if operation == REBOOT && data.is_empty() {
            crate::diagnostics::line(b"management reboot requested");
            self.maintenance(true);
            if self.journal.flush(&mut self.card, self.last_request) {
                self.reboot_at = Some(self.last_request + 1000);
                status = OK;
            } else {
                status = IO_ERROR;
            }
        } else if self.failed {
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
                    crate::diagnostics::line(if operation == BEGIN_KERNEL {
                        b"kernel update started"
                    } else {
                        b"staging upload started"
                    });
                    self.maintenance(true);
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
                            crate::diagnostics::line(b"upload verified and committed");
                            self.maintenance(true);
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
        if status != OK {
            if self.last_error != (operation, status) {
                self.repeated_errors = 0;
            }
            self.last_error = (operation, status);
            self.repeated_errors = self.repeated_errors.saturating_add(1);
            if self.repeated_errors.is_power_of_two() {
                crate::diagnostics::event(format_args!(
                    "management operation={} offset={} failure={} count={}",
                    operation, argument, status, self.repeated_errors
                ));
                self.maintenance(status == IO_ERROR);
            }
        }
        if status == IO_ERROR {
            self.failed = true;
        }
        reply[..4].copy_from_slice(WIRE_MAGIC);
        for (offset, value) in [
            (wire_field::OPERATION, operation | REPLY_OPERATION_BIT),
            (wire_field::SESSION, session),
            (wire_field::ARGUMENT, argument),
            (wire_field::VALUE_OR_STATUS, status),
        ] {
            reply[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        Some(HEADER_LEN + reply_data_len)
    }

    pub fn maintenance(&mut self, critical: bool) {
        let now = crate::raspi2b::timer::uptime_millis();
        self.journal.drain(&mut self.card, now, critical);
        if self.reboot_at.map(|at| now >= at).unwrap_or(false) {
            self.journal.flush(&mut self.card, now);
            crate::raspi2b::reset::reset();
        }
    }

    pub fn transmitted(&mut self, success: bool) {
        if !success {
            self.tx_failures = self.tx_failures.saturating_add(1);
        }
    }

    pub fn network_status(&mut self, session: &crate::network::DhcpSession, link: u32) {
        let put = |bytes: &mut [u8], at: usize, value: u32| {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes())
        };
        put(&mut self.status, 0, 1);
        put(&mut self.status, 4, session.state as u32);
        put(&mut self.status, 8, link);
        self.status[12..16].copy_from_slice(&session.address());
        if let Some(lease) = session.lease {
            self.status[16..20].copy_from_slice(&lease.subnet_mask);
            self.status[20..24].copy_from_slice(&lease.gateway);
            self.status[24..28].copy_from_slice(&lease.server);
            self.status[28..32].copy_from_slice(&lease.dns);
            put(&mut self.status, 60, lease.lease_seconds);
        } else {
            self.status[16..32].fill(0);
            put(&mut self.status, 60, 0);
        }
        self.status[32..38].copy_from_slice(&session.mac_address());
        for (at, value) in [
            (40, session.attempts),
            (44, session.tx_failures),
            (48, session.received_frames),
            (52, session.rejected_dhcp),
            (56, session.lease_losses),
        ] {
            put(&mut self.status, at, value);
        }
        self.status[64..72].copy_from_slice(&session.lease_remaining().to_le_bytes());
    }

    fn fill_storage_status(&mut self) {
        let now = crate::raspi2b::timer::uptime_millis();
        for (at, value) in [
            (72, now),
            (80, self.journal.boot),
            (88, self.journal.sequence),
            (
                96,
                self.journal
                    .sequence
                    .saturating_sub(crate::diagnostics::SECTOR_COUNT - 1)
                    .max(1),
            ),
            (128, self.last_request),
        ] {
            self.status[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        #[cfg(feature = "pi-managed")]
        let (rejected, authenticated) = (self.authentication.rejected, 1);
        #[cfg(not(feature = "pi-managed"))]
        let (rejected, authenticated) = (0, 0);
        for (at, value) in [
            (104, self.journal.write_errors),
            (108, self.journal.dropped_lines),
            (112, self.journal.corrupt_records),
            (116, rejected),
            (120, self.requests),
            (124, self.tx_failures),
            (148, crate::raspi2b::reset::status()),
            (156, gabix_kernel::loader_protocol::MAX_KERNEL_SIZE),
            (160, crate::diagnostics::SECTOR_COUNT as u32),
            (164, authenticated),
        ] {
            self.status[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        if let Some(record) = unsafe { self.card.try_read_sector(crate::sd_install::KERNEL_START) }
            .and_then(|b| {
                gabix_kernel::boot_record::BootRecord::decode(&b, crate::sd_install::KERNEL_SECTORS)
            })
        {
            for (at, value) in [
                (136, record.active_copy as u32),
                (140, record.pending_copy),
                (144, record.attempted_mask),
                (152, record.copies[record.active_copy].image_crc32),
            ] {
                self.status[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
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
        header[disk_field::LENGTH..disk_field::IMAGE_CRC].copy_from_slice(&self.length.to_le_bytes());
        header[disk_field::IMAGE_CRC..disk_field::COMPLETE].copy_from_slice(&self.expected_crc.to_le_bytes());
        header[disk_field::COMPLETE..disk_field::HEADER_CRC].copy_from_slice(&(complete as u32).to_le_bytes());
        let checksum = crc32(&header[..disk_field::HEADER_CRC]);
        header[disk_field::HEADER_CRC..disk_field::HEADER_BYTES].copy_from_slice(&checksum.to_le_bytes());
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
