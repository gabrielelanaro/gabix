//! Minimal 16550 UART (serial port) driver for COM1.
//!
//! QEMU's `-serial stdio` connects the host's stdout to this UART, so
//! anything we write here lands in the terminal we ran `cargo run` in.
//! We mirror all `println!` output to serial so we can capture it to a
//! file — the VGA screen scrolls, but a file does not.

use crate::irq_mutex::IrqMutex;
use core::fmt;
use lazy_static::lazy_static;
use x86_64::instructions::port::Port;

/// I/O port base for COM1 (the first serial port, historically the
/// "modem port"). On PC-compatible hardware this is fixed at 0x3F8.
const COM1_BASE: u16 = 0x3F8;

pub struct SerialPort {
    data: Port<u8>,
}

impl SerialPort {
    pub const fn new() -> Self {
        SerialPort {
            data: Port::new(COM1_BASE),
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        // We skip checking the "transmit holding register empty" status bit.
        // On real hardware at a slow baud rate this could drop bytes when
        // sending faster than the line can transmit. On QEMU's emulated
        // UART, writes are infinitely fast, so it's fine.
        unsafe { self.data.write(byte) };
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

impl gabix_kernel::BootConsole for SerialPort {
    fn write_byte(&mut self, byte: u8) {
        SerialPort::write_byte(self, byte);
    }
}

lazy_static! {
    /// Global handle to COM1. Locked because `_print` writes through it
    /// from any context including interrupt handlers.
    pub static ref SERIAL1: IrqMutex<SerialPort> = IrqMutex::new(SerialPort::new());
}
