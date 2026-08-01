//! Programmable Interval Timer (PIT) — Intel 8253/8254.
//!
//! Configured to fire an interrupt on IRQ 0 at a fixed frequency.
//! The PIT has a base clock of ~1.193182 MHz. We divide that by a 16-bit
//! divisor to get the output frequency. The output goes through the PIC
//! to CPU vector 0x20, where our `timer_handler` runs.

use x86_64::instructions::port::Port;

const CHANNEL_0: u16 = 0x40;
const COMMAND: u16 = 0x43;

/// PIT input clock frequency (the oscillator on the motherboard).
const BASE_FREQUENCY_HZ: u32 = 1193182;

/// Configure channel 0 to fire at `frequency_hz`.
///
/// The divisor is `BASE_FREQUENCY_HZ / frequency_hz` and must fit in 16 bits,
/// so the minimum programmable frequency is ~18.2 Hz. Frequencies above that
/// work fine; very low frequencies overflow the 16-bit divisor.
pub fn set_frequency(frequency_hz: u32) {
    let divisor = BASE_FREQUENCY_HZ / frequency_hz;
    assert!(
        divisor < 0x10000,
        "frequency too low — divisor doesn't fit in 16 bits"
    );

    unsafe {
        let mut cmd: Port<u8> = Port::new(COMMAND);
        let mut chan0: Port<u8> = Port::new(CHANNEL_0);

        // Command byte 0x36:
        //   bits 7-6 = 0b00  → channel 0
        //   bits 5-4 = 0b11  → access mode: lo byte then hi byte
        //   bits 3-1 = 0b010 → mode 2 (rate generator, periodic square wave)
        //   bit 0     = 0    → binary (not BCD)
        cmd.write(0x36);

        // Send divisor low byte, then high byte.
        chan0.write((divisor & 0xFF) as u8);
        chan0.write((divisor >> 8) as u8);
    }
}
