//! BCM2835-family hardware random-number generator.
//! Register contract: Linux drivers/char/hw_random/bcm2835-rng.c.
use core::ptr::{read_volatile, write_volatile};

const BASE: usize = 0x3f10_4000;
const CONTROL: usize = BASE;
const STATUS: usize = BASE + 4;
const DATA: usize = BASE + 8;
const SYSTEM_TIMER_LOW: *const u32 = 0x3f00_3004 as *const u32;

const GENERATOR_ENABLED: u32 = 1;
const WARMUP_SAMPLES: u32 = 0x40000;
const AVAILABLE_WORDS_SHIFT: u32 = 24;
const NONCE_BYTES: usize = 32;
const DATA_WORD_BYTES: usize = core::mem::size_of::<u32>();
const NONCE_TIMEOUT_US: u32 = 1_000_000;

/// Obtain a fresh nonce. A failed generator stops authentication; timestamps
/// and device addresses are unsuitable replacements for random nonce bytes.
pub fn nonce() -> Option<[u8; NONCE_BYTES]> {
    unsafe {
        if read_volatile(CONTROL as *const u32) & GENERATOR_ENABLED == 0 {
            // Hardware discards these initial samples while its source settles.
            write_volatile(STATUS as *mut u32, WARMUP_SAMPLES);
            write_volatile(CONTROL as *mut u32, GENERATOR_ENABLED);
        }
        // One deadline covers all eight words, including the initial warm-up.
        let start = read_volatile(SYSTEM_TIMER_LOW);
        let mut bytes = [0u8; NONCE_BYTES];
        for chunk in bytes.chunks_exact_mut(DATA_WORD_BYTES) {
            // STATUS bits 31:24 count unread 32-bit words in the hardware FIFO.
            // Poll until DATA has a word to consume.
            while read_volatile(STATUS as *const u32) >> AVAILABLE_WORDS_SHIFT == 0 {
                if read_volatile(SYSTEM_TIMER_LOW).wrapping_sub(start) > NONCE_TIMEOUT_US {
                    return None;
                }
                core::hint::spin_loop();
            }
            chunk.copy_from_slice(&read_volatile(DATA as *const u32).to_le_bytes());
        }
        // Reject this obvious failure pattern; this is not a randomness test.
        if bytes.iter().all(|b| *b == 0) {
            return None;
        }
        Some(bytes)
    }
}
