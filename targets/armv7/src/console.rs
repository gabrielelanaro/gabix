use gabix_kernel::BootConsole;

pub struct Pl011 {
    base: *mut u32,
}

impl Pl011 {
    pub const fn new(base: usize) -> Self {
        Self {
            base: base as *mut u32,
        }
    }
}

impl BootConsole for Pl011 {
    fn write_byte(&mut self, byte: u8) {
        unsafe {
            while core::ptr::read_volatile(self.base.add(0x18 / 4)) & 0x20 != 0 {}
            core::ptr::write_volatile(self.base, byte as u32);
        }
    }
}

pub fn write_line(console: &mut impl BootConsole, line: &[u8]) {
    for &byte in line {
        console.write_byte(byte);
    }
    console.write_byte(b'\n');
}
