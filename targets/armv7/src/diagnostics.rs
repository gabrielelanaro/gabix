//! Foreground-only boot log. Console lines queue in RAM; SD writes happen when
//! the service drains this queue, never from an interrupt handler.
use crate::raspi2b::sdhci::SdCard;
use gabix_kernel::{boot_record::crc32, device::WritableBlockDevice};

pub const FIRST_SECTOR: u64 = crate::sd_install::KERNEL_START + 133_120;
pub const SECTOR_COUNT: u64 = 32_768; // Dedicated 16 MiB log area after both kernel copies.
const MAGIC: &[u8; 8] = b"GBXLOG01";
const PAYLOAD: usize = 468;

struct Queue {
    bytes: [u8; 2048],
    used: usize,
    dropped: u32,
}
static mut QUEUE: Queue = Queue {
    bytes: [0; 2048],
    used: 0,
    dropped: 0,
};

pub fn line(text: &[u8]) {
    unsafe {
        let queue = &mut *core::ptr::addr_of_mut!(QUEUE);
        if queue.used + text.len() + 1 > queue.bytes.len() {
            queue.dropped = queue.dropped.saturating_add(1);
            return;
        }
        queue.bytes[queue.used..queue.used + text.len()].copy_from_slice(text);
        queue.used += text.len();
        queue.bytes[queue.used] = b'\n';
        queue.used += 1;
    }
}

pub struct Journal {
    pub boot: u64,
    pub sequence: u64,
    pub write_errors: u32,
    pub dropped_lines: u32,
    pub corrupt_records: u32,
    buffer: [u8; PAYLOAD],
    used: usize,
    last_flush: u64,
}

impl Journal {
    pub fn open(card: &mut SdCard, now: u64) -> Option<Self> {
        let mbr = unsafe { card.try_read_sector(0)? };
        if gabix_kernel::boot_record::find_partition(&mbr, card.sector_count())
            != Some((
                crate::sd_install::KERNEL_START,
                crate::sd_install::KERNEL_SECTORS,
            ))
        {
            // Private-link bootstrap kernels can install the partition first.
            // They must not touch unallocated storage just to start logging.
            return Some(Self {
                boot: 0,
                sequence: 0,
                write_errors: 0,
                dropped_lines: 0,
                corrupt_records: 0,
                buffer: [0; PAYLOAD],
                used: 0,
                last_flush: now,
            });
        }
        let mut latest = 0;
        let mut previous_boot = 0;
        let mut corrupt = 0;
        for index in 0..SECTOR_COUNT {
            if index % 128 == 0 {
                crate::raspi2b::reset::arm_deadman(15);
            }
            let bytes = unsafe { card.try_read_sector(FIRST_SECTOR + index)? };
            if valid_record(&bytes) {
                let sequence = number(&bytes, 8);
                if sequence > latest {
                    latest = sequence;
                    previous_boot = number(&bytes, 16);
                }
            } else if bytes != [0; 512] {
                corrupt += 1;
            }
        }
        let mut journal = Self {
            boot: previous_boot.checked_add(1)?,
            sequence: latest,
            write_errors: 0,
            dropped_lines: 0,
            corrupt_records: corrupt,
            buffer: [0; PAYLOAD],
            used: 0,
            last_flush: now,
        };
        journal.append(card, now, b"boot started\n");
        // Persist a unique boot number before accepting authenticated commands.
        if !journal.flush(card, now) {
            return None;
        }
        Some(journal)
    }

    fn append(&mut self, card: &mut SdCard, now: u64, mut text: &[u8]) {
        while !text.is_empty() {
            if self.used == PAYLOAD && !self.flush(card, now) {
                self.dropped_lines = self.dropped_lines.saturating_add(1);
                return;
            }
            let count = text.len().min(PAYLOAD - self.used);
            self.buffer[self.used..self.used + count].copy_from_slice(&text[..count]);
            self.used += count;
            text = &text[count..];
        }
    }

    pub fn drain(&mut self, card: &mut SdCard, now: u64, critical: bool) {
        unsafe {
            let queue = &mut *core::ptr::addr_of_mut!(QUEUE);
            self.dropped_lines = self.dropped_lines.saturating_add(queue.dropped);
            queue.dropped = 0;
            self.append(card, now, &queue.bytes[..queue.used]);
            queue.used = 0;
        }
        if critical || now.saturating_sub(self.last_flush) >= 1000 {
            self.flush(card, now);
        }
    }

    pub fn flush(&mut self, card: &mut SdCard, now: u64) -> bool {
        if self.boot == 0 {
            self.used = 0;
            return false;
        }
        if self.used == 0 {
            return true;
        }
        let Some(sequence) = self.sequence.checked_add(1) else {
            return false;
        };
        let mut bytes = [0u8; 512];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..16].copy_from_slice(&sequence.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.boot.to_le_bytes());
        bytes[24..32].copy_from_slice(&now.to_le_bytes());
        bytes[32..34].copy_from_slice(&(self.used as u16).to_le_bytes());
        bytes[36..40].copy_from_slice(&self.dropped_lines.to_le_bytes());
        bytes[40..40 + self.used].copy_from_slice(&self.buffer[..self.used]);
        let crc = crc32(&bytes[..508]);
        bytes[508..].copy_from_slice(&crc.to_le_bytes());
        let sector = FIRST_SECTOR + (sequence - 1) % SECTOR_COUNT;
        self.last_flush = now;
        if card.write_sector(sector, &bytes).is_err()
            || unsafe { card.try_read_sector(sector) } != Some(bytes)
        {
            self.write_errors = self.write_errors.saturating_add(1);
            return false;
        }
        self.sequence = sequence;
        self.used = 0;
        true
    }

    pub fn read(&self, card: &mut SdCard, sequence: u64) -> Option<[u8; 512]> {
        if sequence == 0 || sequence > self.sequence || self.sequence - sequence >= SECTOR_COUNT {
            return None;
        }
        let bytes = unsafe { card.try_read_sector(FIRST_SECTOR + (sequence - 1) % SECTOR_COUNT)? };
        (valid_record(&bytes) && number(&bytes, 8) == sequence).then_some(bytes)
    }
}
fn number(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn valid_record(bytes: &[u8; 512]) -> bool {
    &bytes[..8] == MAGIC
        && number(bytes, 8) > 0
        && number(bytes, 16) > 0
        && u16::from_le_bytes(bytes[32..34].try_into().unwrap()) as usize <= PAYLOAD
        && crc32(&bytes[..508]) == u32::from_le_bytes(bytes[508..].try_into().unwrap())
}

pub fn event(arguments: core::fmt::Arguments<'_>) {
    use core::fmt::Write;
    struct Text {
        bytes: [u8; 256],
        used: usize,
    }
    impl Write for Text {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            if self.used + text.len() > self.bytes.len() {
                return Err(core::fmt::Error);
            }
            self.bytes[self.used..self.used + text.len()].copy_from_slice(text.as_bytes());
            self.used += text.len();
            Ok(())
        }
    }
    let mut text = Text {
        bytes: [0; 256],
        used: 0,
    };
    let _ = text.write_fmt(arguments);
    line(&text.bytes[..text.used]);
}
