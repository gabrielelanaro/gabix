//! Programmable Interval Timer (PIT) — Intel 8253/8254.
//!
//! Configured to fire an interrupt on IRQ 0 at a fixed frequency.
//! The PIT has a base clock of ~1.193182 MHz. We divide that by a 16-bit
//! divisor to get the output frequency. The output goes through the PIC
//! to CPU vector 0x20, where our `timer_handler` runs.

use x86_64::instructions::port::Port;

const CHANNEL_0: u16 = 0x40;
const COMMAND: u16 = 0x43;

// Intel 8254 datasheet, Control Word Format: select counter, byte access,
// operating mode, then binary/decimal counting. Combined command remains 0x36.
const SELECT_COUNTER_0: u8 = 0b00 << 6;
const WRITE_LOW_THEN_HIGH_BYTE: u8 = 0b11 << 4;
const MODE_3_SQUARE_WAVE: u8 = 0b011 << 1;
const COUNT_IN_BINARY: u8 = 0;
const PERIODIC_TIMER_COMMAND: u8 =
    SELECT_COUNTER_0 | WRITE_LOW_THEN_HIGH_BYTE | MODE_3_SQUARE_WAVE | COUNT_IN_BINARY;

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
        divisor <= u16::MAX as u32,
        "frequency too low — divisor doesn't fit in 16 bits"
    );

    unsafe {
        let mut command_port: Port<u8> = Port::new(COMMAND);
        let mut counter_0_port: Port<u8> = Port::new(CHANNEL_0);

        // Select counter 0 and periodic square-wave mode. The access bits tell
        // the timer to assemble the next two writes into one 16-bit divisor.
        command_port.write(PERIODIC_TIMER_COMMAND);

        let [low_byte, high_byte] = (divisor as u16).to_le_bytes();
        counter_0_port.write(low_byte);
        counter_0_port.write(high_byte);
    }
}
