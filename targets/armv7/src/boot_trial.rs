//! Boot confirmation happens when the SD-backed service reaches its UART loop.
use crate::raspi2b::{reset, sdhci::SdCard};
use gabix_kernel::boot_record::{
    crc32, find_partition, handoff_field, BootRecord, HANDOFF_ADDRESS, HANDOFF_MAGIC,
};
use gabix_kernel::device::WritableBlockDevice;
const DEVELOPMENT_KERNEL_ADDRESS: usize = 0x0010_0000;
const TRIAL_WATCHDOG_SECONDS: u32 = 15;
// Single-core boot/service state. A failed confirmation keeps the trial active.
static mut TRIAL: Option<(u64, u32)> = None;

/// Consume the RAM handoff once, validate it and match its CRC to this image.
/// Safety: boot-time exclusive access; the handoff and image RAM must be readable.
pub unsafe fn begin() {
    let mut bytes = [0u8; handoff_field::BYTE_LEN];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = core::ptr::read_volatile((HANDOFF_ADDRESS + i) as *const u8);
    }
    // Clear the magic even if validation fails, preventing later reuse.
    core::ptr::write_volatile(HANDOFF_ADDRESS as *mut u32, 0);
    let word = |field: core::ops::Range<usize>| {
        u32::from_le_bytes(bytes[field].try_into().unwrap())
    };
    let length = word(handoff_field::IMAGE_LENGTH) as usize;
    if word(handoff_field::MAGIC) != HANDOFF_MAGIC
        || word(handoff_field::CHECKSUM) != crc32(&bytes[handoff_field::CHECKSUM_INPUT])
        || word(handoff_field::COPY_INDEX) > 1
        || length == 0
        || length > gabix_kernel::loader_protocol::MAX_KERNEL_SIZE as usize
    {
        return;
    }
    // A UART-uploaded kernel must not confirm a stale handoff left by a reset.
    if crc32(core::slice::from_raw_parts(DEVELOPMENT_KERNEL_ADDRESS as *const u8, length)) != word(handoff_field::IMAGE_CRC) {
        return;
    }
    TRIAL = Some((
        u64::from_le_bytes(bytes[handoff_field::RECORD_SECTOR].try_into().unwrap()),
        word(handoff_field::COPY_INDEX),
    ));
    reset::arm_deadman(TRIAL_WATCHDOG_SECONDS);
}
pub fn is_active() -> bool {
    unsafe { (*core::ptr::addr_of!(TRIAL)).is_some() }
}
pub fn refresh() {
    if is_active() {
        reset::arm_deadman(TRIAL_WATCHDOG_SECONDS);
    }
}
/// Confirm only the attempted copy recorded on the same SD partition.
/// Success requires writing the updated record and reading back identical bytes.
/// With no active trial this is a no-op success. On failure the watchdog remains
/// armed; the write may already have reached SD even if read-back fails.
pub fn confirm(card: &mut SdCard) -> bool {
    let Some((sector, index)) = (unsafe { *core::ptr::addr_of!(TRIAL) }) else {
        return true;
    };
    let result = (|| unsafe {
        let mbr = card.try_read_sector(0)?;
        let (start, count) = find_partition(&mbr, card.sector_count())?;
        if start != sector {
            return None;
        }
        let mut record = BootRecord::decode(&card.try_read_sector(start)?, count)?;
        if !record.confirm(index) {
            return None;
        }
        let bytes = record.encode();
        card.write_sector(start, &bytes).ok()?;
        (card.try_read_sector(start)? == bytes).then_some(())
    })();
    if result.is_some() {
        unsafe {
            TRIAL = None;
        }
        reset::stop_deadman();
        true
    } else {
        false
    }
}
