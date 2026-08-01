#![no_std]

pub mod device;
pub mod memory;

/// Byte-oriented console supplied by a platform adapter.
pub trait BootConsole {
    fn write_byte(&mut self, byte: u8);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    X86Qemu,
    ArmVirt,
    RaspberryPi2B,
}

impl Platform {
    pub const fn boot_message(self) -> &'static [u8] {
        match self {
            Platform::X86Qemu => b"gabix x86_64 booting\n",
            Platform::ArmVirt => b"gabix arm virt booted\n",
            Platform::RaspberryPi2B => b"gabix raspi2b booted\n",
        }
    }
}

/// First architecture-neutral kernel operation.
pub fn announce_boot(console: &mut impl BootConsole, platform: Platform) {
    for &byte in platform.boot_message() {
        console.write_byte(byte);
    }
}
