//! PCI (Peripheral Component Interconnect) bus access.
//!
//! Configuration access, bus-0 discovery, and Base Address Register decoding.
//! The configuration address/data port pair must be accessed serially.
//!
//! The PCI standard reserves two 32-bit I/O ports for config space access:
//!   - port 0xCF8: write a 32-bit address word identifying bus/device/func/offset
//!   - port 0xCFC: read or write 32 bits of data at that address
//!
//! See Lesson 5 for the bitfield diagram of the address word.

use crate::println;
use x86_64::instructions::port::Port;

/// Coordinates of one function on the PCI bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub func: u8,
}

/// I/O port for the PCI configuration address (32-bit).
const CONFIG_ADDRESS: u16 = 0xCF8;

/// I/O port for PCI configuration data (32-bit).
const CONFIG_DATA: u16 = 0xCFC;

// Conventional PCI configuration layout; see Linux include/uapi/linux/pci_regs.h.
const ROOT_BUS: u8 = 0;
const DEVICES_PER_BUS: u8 = 32;
const FUNCTIONS_PER_DEVICE: u8 = 8;
const VENDOR_DEVICE_IDS: u8 = 0x00;
const CLASS_REVISION: u8 = 0x08;
const FIRST_BAR: u8 = 0x10;
const CONFIG_WORD_BYTES: u8 = 4;
const ENDPOINT_BAR_COUNT: u8 = 6;
const ABSENT_VENDOR: u16 = 0xffff;
const BAR_IO_SPACE: u32 = 1;
const BAR_IO_ADDRESS_MASK: u32 = !0x3;
const BAR_MEMORY_ADDRESS_MASK: u32 = !0xf;
const BAR_MEMORY_TYPE_SHIFT: u32 = 1;
const BAR_MEMORY_TYPE_MASK: u32 = 0b11;
const BAR_MEMORY_32_BIT: u32 = 0b00;
const BAR_MEMORY_64_BIT: u32 = 0b10;
const BAR_PREFETCHABLE: u32 = 1 << 3;

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

/// Read the base physical address encoded in BAR number `bar_index`.
///
/// For 32-bit memory BARs, returns the address in the low 32 bits with the
/// type bits masked off. For 64-bit memory BARs, combines the next BAR slot
/// to form a 64-bit address. For I/O BARs, returns the address from the low
/// 32 bits (treated as a u64 — callers can downcast if they know it's small).
///
/// Returns 0 for an unused BAR (one that reads as all zeros).
pub fn bar_base(bus: u8, device: u8, func: u8, bar_index: u8) -> u64 {
    let offset = FIRST_BAR + bar_index * CONFIG_WORD_BYTES;
    let bar = config_read(bus, device, func, offset);

    if bar == 0 {
        return 0;
    }

    if bar & BAR_IO_SPACE != 0 {
        // I/O BAR: bit 0 selects I/O, bit 1 is reserved, remaining bits form the address.
        return (bar & BAR_IO_ADDRESS_MASK) as u64;
    }

    // Memory BAR
    let type_bits = (bar >> BAR_MEMORY_TYPE_SHIFT) & BAR_MEMORY_TYPE_MASK;
    let base_low = (bar & BAR_MEMORY_ADDRESS_MASK) as u64;

    if type_bits == BAR_MEMORY_64_BIT {
        // 64-bit memory BAR — combine with the next BAR slot's high 32 bits
        let next_offset = FIRST_BAR + (bar_index + 1) * CONFIG_WORD_BYTES;
        let bar_high = config_read(bus, device, func, next_offset);
        base_low | ((bar_high as u64) << 32)
    } else {
        base_low
    }
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
/// Used for PCI command fields such as bus mastering and for BAR addresses.
/// VirtIO device status is accessed through its transport-specific registers.
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
    for device in 0..DEVICES_PER_BUS {
        for func in 0..FUNCTIONS_PER_DEVICE {
            // Reading offset 0x00 returns vendor_id (low 16) | device_id (high 16)
            let vendor_word = config_read(ROOT_BUS, device, func, VENDOR_DEVICE_IDS);
            let vendor_id = (vendor_word & 0xFFFF) as u16;
            if vendor_id == ABSENT_VENDOR {
                continue; // absent slot
            }
            let device_id = (vendor_word >> 16) as u16;

            // Reading offset 0x08 returns revision (low 8) | prog_if (8) |
            // subclass (8) | base_class (high 8). We extract base_class for
            // the human-readable name.
            let class_word = config_read(ROOT_BUS, device, func, CLASS_REVISION);
            let base_class = ((class_word >> 24) & 0xFF) as u8;
            let subclass = ((class_word >> 16) & 0xFF) as u8;

            println!(
                "[pci] {:02x}:{:02x}.{} vendor={:04x} device={:04x} class={:02x}{:02x} ({})",
                ROOT_BUS,
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

/// PCI vendor ID for Red Hat / virtio devices.
const VIRTIO_VENDOR_ID: u16 = 0x1AF4;

/// Transitional virtio-blk device ID — supports both legacy and modern transports.
const VIRTIO_BLK_TRANSITIONAL: u16 = 0x1001;

/// Non-transitional (modern-only) virtio-blk device ID.
pub const VIRTIO_BLK_NON_TRANSITIONAL: u16 = 0x1042;

/// Non-transitional (modern-only) virtio-net device ID.
pub const VIRTIO_NET_NON_TRANSITIONAL: u16 = 0x1041;

/// Search bus 0 for one modern virtio device ID.
pub fn find_virtio_device(device_id: u16) -> Option<PciDevice> {
    for device in 0..DEVICES_PER_BUS {
        for func in 0..FUNCTIONS_PER_DEVICE {
            let vendor_word = config_read(ROOT_BUS, device, func, VENDOR_DEVICE_IDS);
            let vendor_id = (vendor_word & 0xFFFF) as u16;
            if vendor_id != VIRTIO_VENDOR_ID {
                continue;
            }
            let found_device_id = (vendor_word >> 16) as u16;
            if found_device_id == device_id {
                return Some(PciDevice {
                    bus: ROOT_BUS,
                    device,
                    func,
                });
            }
        }
    }
    None
}

/// Search bus 0 for the virtio-blk device.
///
/// Returns `Some((bus, device, func))` if found, `None` if no virtio-blk
/// device is attached. Accepts both transitional (0x1001) and non-transitional
/// (0x1042) device IDs — QEMU exposes transitional by default.
pub fn find_virtio_blk() -> Option<(u8, u8, u8)> {
    if let Some(dev) = find_virtio_device(VIRTIO_BLK_NON_TRANSITIONAL) {
        return Some((dev.bus, dev.device, dev.func));
    }

    for device in 0..DEVICES_PER_BUS {
        for func in 0..FUNCTIONS_PER_DEVICE {
            let vendor_word = config_read(ROOT_BUS, device, func, VENDOR_DEVICE_IDS);
            let vendor_id = (vendor_word & 0xFFFF) as u16;
            if vendor_id != VIRTIO_VENDOR_ID {
                continue;
            }
            let device_id = (vendor_word >> 16) as u16;
            if device_id == VIRTIO_BLK_TRANSITIONAL {
                return Some((ROOT_BUS, device, func));
            }
        }
    }
    None
}

/// Read all six BAR slots of a normal endpoint (PCI header type 0).
/// Callers must supply an endpoint; bridge headers have a different layout.
///
/// BAR layout per slot (32 bits):
///   - bit 0 = 1: I/O BAR. Bits 2-31 = base address (mask off low 2 type bits).
///   - bit 0 = 0: memory BAR.
///       - bits 1-2 = type: 00 = 32-bit, 10 = 64-bit (uses TWO BAR slots),
///                              01 = obsolete below-1-MiB address, 11 = reserved.
///       - bit 3 = prefetchable flag (1 = safe to read-ahead).
///       - bits 4-31 = base address (mask off low 4 type bits).
///
/// For 64-bit memory BARs, the next BAR slot holds the high 32 bits of the
/// base address — combined with this slot's low 32 bits to form a 64-bit base.
pub fn print_bars(bus: u8, device: u8, func: u8) {
    let mut i = 0u8;
    while i < ENDPOINT_BAR_COUNT {
        let offset = FIRST_BAR + i * CONFIG_WORD_BYTES;
        let bar = config_read(bus, device, func, offset);

        if bar == 0 {
            println!("[pci]   BAR{} = 0x{:08X} (unused)", i, bar);
            i += 1;
            continue;
        }

        if bar & BAR_IO_SPACE != 0 {
            // I/O BAR
            let base = bar & BAR_IO_ADDRESS_MASK;
            println!(
                "[pci]   BAR{} = 0x{:08X} (I/O space, base 0x{:08X})",
                i, bar, base
            );
            i += 1;
        } else {
            // Memory BAR
            let type_bits = (bar >> BAR_MEMORY_TYPE_SHIFT) & BAR_MEMORY_TYPE_MASK;
            let prefetchable = (bar & BAR_PREFETCHABLE) != 0;
            let base_low = bar & BAR_MEMORY_ADDRESS_MASK;
            let pref_str = if prefetchable { " prefetchable" } else { "" };

            if type_bits == BAR_MEMORY_64_BIT {
                // 64-bit memory BAR — consumes this slot AND the next
                let next_offset = offset + CONFIG_WORD_BYTES;
                let bar_high = config_read(bus, device, func, next_offset);
                let base64 = (base_low as u64) | ((bar_high as u64) << 32);
                println!(
                    "[pci]   BAR{}-BAR{} = 0x{:08X}{:08X} (memory, 64-bit{}, base 0x{:016X})",
                    i,
                    i + 1,
                    bar_high,
                    bar,
                    pref_str,
                    base64
                );
                i += 2; // consume both slots
            } else if type_bits == BAR_MEMORY_32_BIT {
                // 32-bit memory BAR
                println!(
                    "[pci]   BAR{} = 0x{:08X} (memory, 32-bit{}, base 0x{:08X})",
                    i, bar, pref_str, base_low
                );
                i += 1;
            } else {
                // This diagnostic labels both unsupported encodings (01 and 11) reserved.
                println!(
                    "[pci]   BAR{} = 0x{:08X} (memory, RESERVED type {})",
                    i, bar, type_bits
                );
                i += 1;
            }
        }
    }
}
