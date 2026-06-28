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
