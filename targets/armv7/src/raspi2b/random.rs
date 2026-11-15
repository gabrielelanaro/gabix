//! BCM2835-family hardware random-number generator.
//! Register contract: Linux drivers/char/hw_random/bcm2835-rng.c.
use core::ptr::{read_volatile, write_volatile};

const BASE: usize = 0x3f10_4000;
const CONTROL: usize = BASE;
const STATUS: usize = BASE + 4;
const DATA: usize = BASE + 8;
const TIMER: *const u32 = 0x3f00_3004 as *const u32;

/// Obtain a fresh nonce. A failed generator stops authentication; timestamps
/// and device addresses are unsuitable replacements for random nonce bytes.
pub fn nonce() -> Option<[u8; 32]> {
    unsafe {
        if read_volatile(CONTROL as *const u32) & 1 == 0 {
            // Hardware discards these initial samples while its source settles.
            write_volatile(STATUS as *mut u32, 0x40000);
            write_volatile(CONTROL as *mut u32, 1);
        }
        let start = read_volatile(TIMER);
        let mut bytes = [0u8; 32];
        for chunk in bytes.chunks_exact_mut(4) {
            while read_volatile(STATUS as *const u32) >> 24 == 0 {
                if read_volatile(TIMER).wrapping_sub(start) > 1_000_000 {
                    return None;
                }
                core::hint::spin_loop();
            }
            chunk.copy_from_slice(&read_volatile(DATA as *const u32).to_le_bytes());
        }
        if bytes.iter().all(|b| *b == 0) {
            return None;
        }
        Some(bytes)
    }
}
