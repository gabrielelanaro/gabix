use gabix_kernel::BootConsole;

// Register offsets below are word indices because base points to u32.
const UART_FLAGS_WORD: usize = 0x18 / core::mem::size_of::<u32>();
const TX_FIFO_FULL: u32 = 1 << 5;
#[cfg(feature = "pi-uart-user")]
const RX_FIFO_EMPTY: u32 = 1 << 4;

/// Polling access to a PL011 already configured by the boot code.
/// Multiple handles do not serialize access; callers own that coordination.
pub struct Pl011 {
    base: *mut u32,
}

impl Pl011 {
    /// Caller must supply an accessible, aligned PL011 register base.
    /// Construction performs no device discovery or register initialization.
    pub const fn new(base: usize) -> Self {
        Self {
            base: base as *mut u32,
        }
    }

    #[cfg(feature = "pi-uart-user")]
    /// Read at most one queued byte. Receive error bits are discarded.
    pub fn try_read_byte(&mut self) -> Option<u8> {
        unsafe {
            if core::ptr::read_volatile(self.base.add(UART_FLAGS_WORD)) & RX_FIFO_EMPTY != 0 {
                None
            } else {
                Some(core::ptr::read_volatile(self.base) as u8)
            }
        }
    }
}

impl BootConsole for Pl011 {
    // Wait without a timeout for FIFO room, then enqueue the byte. This does
    // not wait for the final serial bit to leave the UART.
    fn write_byte(&mut self, byte: u8) {
        unsafe {
            while core::ptr::read_volatile(self.base.add(UART_FLAGS_WORD)) & TX_FIFO_FULL != 0 {}
            core::ptr::write_volatile(self.base, byte as u32);
        }
    }
}

pub fn write_line(console: &mut impl BootConsole, line: &[u8]) {
    #[cfg(feature = "pi-sd-transfer")]
    crate::diagnostics::line(line);
    for &byte in line {
        console.write_byte(byte);
    }
    console.write_byte(b'\n');
}
