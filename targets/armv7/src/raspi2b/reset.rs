//! Board reset through the power manager's watchdog countdown.
//! Reference sequence and fields: Linux drivers/watchdog/bcm2835_wdt.c.

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

const POWER_BASE: usize = 0x3f10_0000;
const RESET_CONTROL: *mut u32 = (POWER_BASE + 0x1c) as *mut u32;
const RESET_STATUS: *const u32 = (POWER_BASE + 0x20) as *const u32;
const WATCHDOG: *mut u32 = (POWER_BASE + 0x24) as *mut u32;
// Power-manager writes include this required key in their high byte.
const REGISTER_WRITE_KEY: u32 = 0x5a00_0000;
const RESET_CONFIG_BITS: u32 = 0x30; // PM_RSTC.WRCFG bits 5:4.
const FULL_RESET: u32 = 0x20;
// PM_RSTC_RESET: the reference watchdog-stop control word.
const STOP_RESET: u32 = 0x102;
const WATCHDOG_TICKS_MASK: u32 = 0x000f_ffff;

const WATCHDOG_TICKS_PER_SECOND: u32 = 1 << 16;
const REBOOT_DELAY_TICKS: u32 = 10; // Approximately 150 microseconds.

/// Start or refresh the reset countdown. Callers use 1..=15 whole seconds:
/// the hardware time field holds 20 bits, and this function masks excess bits
/// rather than clamping to the maximum duration.
pub fn arm_deadman(seconds: u32) {
    let ticks = seconds.saturating_mul(WATCHDOG_TICKS_PER_SECOND) & WATCHDOG_TICKS_MASK;
    unsafe {
        // Load the countdown before selecting full-board reset on expiry.
        write_volatile(WATCHDOG, REGISTER_WRITE_KEY | ticks);
        let control = read_volatile(RESET_CONTROL);
        write_volatile(
            RESET_CONTROL,
            REGISTER_WRITE_KEY | (control & !RESET_CONFIG_BITS) | FULL_RESET,
        );
        asm!("dsb sy", options(nostack, preserves_flags));
    }
}

/// Cancel watchdog-triggered reset after the protected operation succeeds.
pub fn stop_deadman() {
    unsafe {
        write_volatile(RESET_CONTROL, REGISTER_WRITE_KEY | STOP_RESET);
        asm!("dsb sy", options(nostack, preserves_flags));
    }
}

/// Ask the BCM2836 power manager to reset the board through its watchdog.
pub fn reset() -> ! {
    unsafe {
        write_volatile(WATCHDOG, REGISTER_WRITE_KEY | REBOOT_DELAY_TICKS);
        let control = read_volatile(RESET_CONTROL);
        write_volatile(
            RESET_CONTROL,
            REGISTER_WRITE_KEY | (control & !RESET_CONFIG_BITS) | FULL_RESET,
        );
        asm!("dsb sy", options(nostack, preserves_flags));
        loop {
            asm!("wfe", options(nomem, nostack, preserves_flags));
        }
    }
}

/// Raw PM_RSTS value; callers retain the register bits without guessing a cause.
pub fn status() -> u32 {
    unsafe { read_volatile(RESET_STATUS) }
}
