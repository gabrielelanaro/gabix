//! PCI (Peripheral Component Interconnect) bus access.
//!
//! Lesson 5 step 5.2: raw config space access via I/O ports 0xCF8 (address)
//! and 0xCFC (data). No device discovery yet — that's step 5.3.
//!
//! The PCI standard reserves two 32-bit I/O ports for config space access:
//!   - port 0xCF8: write a 32-bit address word identifying bus/device/func/offset
//!   - port 0xCFC: read or write 32 bits of data at that address
//!
//! See Lesson 5 for the bitfield diagram of the address word.

use crate::println;
use x86_64::instructions::port::Port;

/// I/O port for the PCI configuration address (32-bit).
const CONFIG_ADDRESS: u16 = 0xCF8;

/// I/O port for PCI configuration data (32-bit).
const CONFIG_DATA: u16 = 0xCFC;

/// Build the 32-bit PCI config address word.
///
/// Bit layout:
///   bit 31    : enable (must be 1 or the access is ignored)
///   bits 30-24: reserved (must be 0)
///   bits 23-16: bus number (0-255)
///   bits 15-11: device number (0-31)
///   bits 10-8 : function number (0-7)
///   bits 7-2  : register offset (4-byte aligned — low 2 bits are masked off)
///   bits 1-0  : always 0 (32-bit access)
fn make_address(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    (1u32 << 31)
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC)
}

/// Read 32 bits from PCI config space at the given bus/device/function/offset.
///
/// Offset must be 4-byte aligned (the low 2 bits are masked off automatically).
/// Reading from a non-existent device returns `0xFFFFFFFF`.
pub fn config_read(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    let address = make_address(bus, device, func, offset);
    unsafe {
        let mut addr_port: Port<u32> = Port::new(CONFIG_ADDRESS);
        let mut data_port: Port<u32> = Port::new(CONFIG_DATA);
        addr_port.write(address);
        data_port.read()
    }
}

/// Write 32 bits to PCI config space at the given bus/device/function/offset.
///
/// Used for enabling bus mastering in the command register, assigning BAR
/// addresses, setting virtio device status bits (DRIVER_OK in Lesson 6),
/// etc. Writes to read-only fields (vendor_id, device_id, class_code) are
/// silently ignored by the device.
pub fn config_write(bus: u8, device: u8, func: u8, offset: u8, value: u32) {
    let address = make_address(bus, device, func, offset);
    unsafe {
        let mut addr_port: Port<u32> = Port::new(CONFIG_ADDRESS);
        let mut data_port: Port<u32> = Port::new(CONFIG_DATA);
        addr_port.write(address);
        data_port.write(value);
    }
}

/// Look up the human-readable name for a base class code.
///
/// The base class is the top-level "what kind of device is this" — disk,
/// network, display, bridge, etc. Full table at
/// https://pci-ids.ucw.cz/read/PD/ — here we just include the common ones.
fn class_name(base_class: u8) -> &'static str {
    match base_class {
        0x00 => "unclassified",
        0x01 => "mass storage",
        0x02 => "network",
        0x03 => "display",
        0x04 => "multimedia",
        0x05 => "memory controller",
        0x06 => "bridge",
        0x07 => "comm controller",
        0x08 => "generic peripheral",
        0x09 => "input device",
        0x0B => "processor",
        0x0C => "serial bus (USB etc.)",
        0x0D => "wireless",
        _ => "other",
    }
}

/// Walk bus 0, every device (0-31), every function (0-7). For each present
/// slot (vendor != 0xFFFF), print the bus:device.function coordinates, the
/// vendor/device IDs, and the class code.
///
/// QEMU's default pc topology puts everything on bus 0, so we don't need to
/// scan buses 1-255. Real PCI bridges can fan out to subordinate buses —
/// Linux's `pci_scan_bus` recurses; we don't need that complexity here.
pub fn enumerate() {
    println!("[pci] scanning bus 0");
    for device in 0..32u8 {
        for func in 0..8u8 {
            // Reading offset 0x00 returns vendor_id (low 16) | device_id (high 16)
            let vendor_word = config_read(0, device, func, 0x00);
            let vendor_id = (vendor_word & 0xFFFF) as u16;
            if vendor_id == 0xFFFF {
                continue; // absent slot
            }
            let device_id = (vendor_word >> 16) as u16;

            // Reading offset 0x08 returns revision (low 8) | prog_if (8) |
            // subclass (8) | base_class (high 8). We extract base_class for
            // the human-readable name.
            let class_word = config_read(0, device, func, 0x08);
            let base_class = ((class_word >> 24) & 0xFF) as u8;
            let subclass = ((class_word >> 16) & 0xFF) as u8;

            println!(
                "[pci] {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x}{:02x} ({})",
                0u8,
                device,
                func,
                vendor_id,
                device_id,
                base_class,
                subclass,
                class_name(base_class)
            );
        }
    }
}
