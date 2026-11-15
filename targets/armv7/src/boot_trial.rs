//! Boot confirmation happens when the SD-backed service reaches its UART loop.
use crate::raspi2b::{reset, sdhci::SdCard};
use gabix_kernel::boot_record::{
    crc32, find_partition, BootRecord, HANDOFF_ADDRESS, HANDOFF_MAGIC,
};
use gabix_kernel::device::WritableBlockDevice;
static mut TRIAL: Option<(u64, u32)> = None;

pub unsafe fn begin() {
    let mut bytes = [0u8; 28];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = core::ptr::read_volatile((HANDOFF_ADDRESS + i) as *const u8);
    }
    core::ptr::write_volatile(HANDOFF_ADDRESS as *mut u32, 0);
    let word = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let length = word(16) as usize;
    if word(0) != HANDOFF_MAGIC
        || word(24) != crc32(&bytes[..24])
        || word(12) > 1
        || length == 0
        || length > gabix_kernel::loader_protocol::MAX_KERNEL_SIZE as usize
    {
        return;
    }
    // A UART-uploaded kernel must not confirm a stale handoff left by a reset.
    if crc32(core::slice::from_raw_parts(0x00100000 as *const u8, length)) != word(20) {
        return;
    }
    TRIAL = Some((
        u64::from_le_bytes(bytes[4..12].try_into().unwrap()),
        word(12),
    ));
    reset::arm_deadman(15);
}
pub fn is_active() -> bool {
    unsafe { (*core::ptr::addr_of!(TRIAL)).is_some() }
}
pub fn refresh() {
    if is_active() {
        reset::arm_deadman(15);
    }
}
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
