use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

const POWER_BASE: usize = 0x3f10_0000;
const RESET_CONTROL: *mut u32 = (POWER_BASE + 0x1c) as *mut u32;
const WATCHDOG: *mut u32 = (POWER_BASE + 0x24) as *mut u32;
const PASSWORD: u32 = 0x5a00_0000;
const RESET_CONFIG_MASK: u32 = 0xffff_ffcf;
const FULL_RESET: u32 = 0x20;
const STOP_RESET: u32 = 0x102;
const WATCHDOG_TICKS_MASK: u32 = 0x000f_ffff;

pub fn arm_deadman(seconds: u32) {
    let ticks = seconds.saturating_mul(1 << 16) & WATCHDOG_TICKS_MASK;
    unsafe {
        write_volatile(WATCHDOG, PASSWORD | ticks);
        let control = read_volatile(RESET_CONTROL);
        write_volatile(
            RESET_CONTROL,
            PASSWORD | (control & RESET_CONFIG_MASK) | FULL_RESET,
        );
        asm!("dsb sy", options(nostack, preserves_flags));
    }
}

pub fn stop_deadman() {
    unsafe {
        write_volatile(RESET_CONTROL, PASSWORD | STOP_RESET);
        asm!("dsb sy", options(nostack, preserves_flags));
    }
}

/// Ask the BCM2836 power manager to reset the board through its watchdog.
pub fn reset() -> ! {
    unsafe {
        write_volatile(WATCHDOG, PASSWORD | 10);
        let control = read_volatile(RESET_CONTROL);
        write_volatile(
            RESET_CONTROL,
            PASSWORD | (control & RESET_CONFIG_MASK) | FULL_RESET,
        );
        asm!("dsb sy", options(nostack, preserves_flags));
        loop {
            asm!("wfe", options(nomem, nostack, preserves_flags));
        }
    }
}

/// Raw PM_RSTS value; callers retain the register bits without guessing a cause.
pub fn status() -> u32 {
    unsafe { read_volatile((POWER_BASE + 0x20) as *const u32) }
}
