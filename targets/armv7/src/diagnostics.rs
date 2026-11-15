//! Foreground-only boot log. Console lines queue in RAM; SD writes happen when
//! the service drains this queue, never from an interrupt handler.
use crate::raspi2b::sdhci::SdCard;
use gabix_kernel::{boot_record::crc32, device::WritableBlockDevice};

pub const FIRST_SECTOR: u64 = crate::sd_install::KERNEL_START + 133_120;
pub const SECTOR_COUNT: u64 = 32_768; // Dedicated 16 MiB log area after both kernel copies.
const MAGIC: &[u8; 8] = b"GBXLOG01";
const QUEUE_BYTES: usize = 2048;
const EVENT_TEXT_BYTES: usize = 256;
const ROUTINE_FLUSH_INTERVAL_MS: u64 = 1000;
const SCAN_WATCHDOG_INTERVAL_SECTORS: u64 = 128;
const SCAN_WATCHDOG_SECONDS: u32 = 15;
// One little-endian record occupies one SD sector. Bytes 34..36 are reserved.
// Layout is also consumed by scripts/pi2-manage.py::logs.
mod record_field {
    pub const SEQUENCE: usize = 8;
    pub const BOOT: usize = 16;
    pub const UPTIME_MS: usize = 24;
    pub const LENGTH: core::ops::Range<usize> = 32..34;
    pub const DROPPED_LINES: core::ops::Range<usize> = 36..40;
    pub const TEXT: usize = 40;
    pub const CRC: usize = 508;
}
const PAYLOAD: usize = record_field::CRC - record_field::TEXT;

struct Queue {
    bytes: [u8; QUEUE_BYTES],
    used: usize,
    dropped: u32,
}
static mut QUEUE: Queue = Queue {
    bytes: [0; QUEUE_BYTES],
    used: 0,
    dropped: 0,
};

/// Queue a whole line plus newline, or count it as dropped when RAM is full.
/// Caller must serialize foreground access: this static queue has no lock.
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
    /// Scan all log sectors, recover the highest valid sequence, and persist a
    /// new boot number. Unreadable sectors abort opening; invalid nonzero records
    /// are counted and skipped. A missing kernel partition yields boot=0 disabled
    /// logging, which the managed receiver refuses for authenticated service.
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
            if index % SCAN_WATCHDOG_INTERVAL_SECTORS == 0 {
                crate::raspi2b::reset::arm_deadman(SCAN_WATCHDOG_SECONDS);
            }
            let bytes = unsafe { card.try_read_sector(FIRST_SECTOR + index)? };
            if valid_record(&bytes) {
                let sequence = number(&bytes, record_field::SEQUENCE);
                if sequence > latest {
                    latest = sequence;
                    previous_boot = number(&bytes, record_field::BOOT);
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

    // Split the byte stream across records, even in the middle of a line/UTF-8
    // character. A failed full-buffer flush drops the remaining input batch and
    // increments dropped_lines once; that counter is not an exact lost-line total.
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

    /// Move queued bytes into the journal buffer, then flush when critical or due.
    /// Caller must serialize queue access. This returns no success status; inspect
    /// counters or call flush explicitly when confirmation is required.
    pub fn drain(&mut self, card: &mut SdCard, now: u64, critical: bool) {
        unsafe {
            let queue = &mut *core::ptr::addr_of_mut!(QUEUE);
            self.dropped_lines = self.dropped_lines.saturating_add(queue.dropped);
            queue.dropped = 0;
            self.append(card, now, &queue.bytes[..queue.used]);
            queue.used = 0;
        }
        if critical || now.saturating_sub(self.last_flush) >= ROUTINE_FLUSH_INTERVAL_MS {
            self.flush(card, now);
        }
    }

    /// Write one ring record and require byte-for-byte read-back before advancing.
    /// Failure retains buffered text but advances last_flush, delaying routine
    /// retries; the disk write may already have succeeded. boot=0 discards text.
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
        bytes[record_field::SEQUENCE..record_field::BOOT].copy_from_slice(&sequence.to_le_bytes());
        bytes[record_field::BOOT..record_field::UPTIME_MS]
            .copy_from_slice(&self.boot.to_le_bytes());
        bytes[record_field::UPTIME_MS..record_field::LENGTH.start]
            .copy_from_slice(&now.to_le_bytes());
        bytes[record_field::LENGTH].copy_from_slice(&(self.used as u16).to_le_bytes());
        bytes[record_field::DROPPED_LINES].copy_from_slice(&self.dropped_lines.to_le_bytes());
        bytes[record_field::TEXT..record_field::TEXT + self.used]
            .copy_from_slice(&self.buffer[..self.used]);
        let crc = crc32(&bytes[..record_field::CRC]);
        bytes[record_field::CRC..].copy_from_slice(&crc.to_le_bytes());
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
        (valid_record(&bytes) && number(&bytes, record_field::SEQUENCE) == sequence)
            .then_some(bytes)
    }
}
fn number(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn valid_record(bytes: &[u8; 512]) -> bool {
    &bytes[..8] == MAGIC
        && number(bytes, record_field::SEQUENCE) > 0
        && number(bytes, record_field::BOOT) > 0
        && u16::from_le_bytes(bytes[record_field::LENGTH].try_into().unwrap()) as usize <= PAYLOAD
        && crc32(&bytes[..record_field::CRC])
            == u32::from_le_bytes(bytes[record_field::CRC..].try_into().unwrap())
}

/// Format into fixed RAM storage, then queue the completed prefix. Formatting
/// overflow is ignored, so truncated events have no separate truncation counter.
pub fn event(arguments: core::fmt::Arguments<'_>) {
    use core::fmt::Write;
    struct Text {
        bytes: [u8; EVENT_TEXT_BYTES],
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
        bytes: [0; EVENT_TEXT_BYTES],
        used: 0,
    };
    let _ = text.write_fmt(arguments);
    line(&text.bytes[..text.used]);
}
