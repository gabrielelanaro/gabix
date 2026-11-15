#![no_std]

#[cfg(feature = "kernel")]
extern crate alloc;

#[cfg(feature = "kernel")]
pub mod device;
#[cfg(feature = "kernel")]
pub mod elf;
#[cfg(feature = "kernel")]
pub mod fs;
pub mod loader_protocol;
pub mod boot_record;
#[cfg(feature = "kernel")]
pub mod memory;
#[cfg(feature = "kernel")]
pub mod net;

/// Byte-oriented console supplied by a platform adapter.
#[cfg(feature = "kernel")]
pub trait BootConsole {
    fn write_byte(&mut self, byte: u8);
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    X86Qemu,
    ArmVirt,
    RaspberryPi2B,
}

#[cfg(feature = "kernel")]
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
#[cfg(feature = "kernel")]
pub fn announce_boot(console: &mut impl BootConsole, platform: Platform) {
    for &byte in platform.boot_message() {
        console.write_byte(byte);
    }
}
