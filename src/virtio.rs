//! virtio device driver — Lesson 6: capability list parsing and device init.
//!
//! Lesson 6 step 6.1: walk the PCI capability list of the virtio-blk device,
//! parse the virtio-specific vendor capabilities, and print each region's
//! location (BAR, offset, length).
//!
//! The PCI capability list lives in the upper half of config space (offsets
//! 0x40-0xFF). Each capability is a small header (cap_id, next_ptr, length)
//! followed by capability-specific body. virtio uses vendor-specific caps
//! (cap_id = 0x09) with a sub-header that points at a memory region inside
//! one of the device's BARs.
//!
//! The list is walked by reading the "capabilities pointer" at offset 0x34
//! in the standard config header, then following next_ptr until it's 0.

use crate::println;
use crate::pci;

/// Offset of the capabilities pointer in PCI config space.
const CAPABILITIES_POINTER_OFFSET: u8 = 0x34;

/// Capability ID for "vendor specific" — virtio uses this type.
const CAP_VENDOR_SPECIFIC: u8 = 0x09;

/// virtio vendor-specific capability sub-types (the `cfg_type` field).
const VIRTIO_CAP_COMMON: u8 = 1;
const VIRTIO_CAP_NOTIFY: u8 = 2;
const VIRTIO_CAP_ISR: u8 = 3;
const VIRTIO_CAP_DEVICE_CONFIG: u8 = 4;
const VIRTIO_CAP_PCI_CONFIG: u8 = 5;

/// One parsed virtio vendor-specific capability.
///
/// Says: "structure of type `cfg_type` lives at `bar` + `offset`, for
/// `length` bytes." The driver translates (bar, offset) into a virtual
/// address (via the BAR's base address plus the kernel's MMIO mapping)
/// and accesses the structure via normal memory loads/stores.
#[derive(Debug, Clone, Copy)]
pub struct VirtioCap {
    pub cfg_type: u8,
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
}

/// Human-readable label for a virtio capability sub-type.
fn cap_type_name(cfg_type: u8) -> &'static str {
    match cfg_type {
        VIRTIO_CAP_COMMON => "common config",
        VIRTIO_CAP_NOTIFY => "notifications",
        VIRTIO_CAP_ISR => "ISR status",
        VIRTIO_CAP_DEVICE_CONFIG => "device config",
        VIRTIO_CAP_PCI_CONFIG => "PCI config (alt)",
        _ => "unknown",
    }
}

/// Walk the PCI capability list of the device at `bus`:`device`:`func`,
/// collecting every virtio vendor-specific capability found.
///
/// Non-vendor-specific caps (Power Management, MSI, MSI-X, etc.) are skipped.
/// Returns the list of virtio caps, in the order they appear in the list.
pub fn parse_virtio_capabilities(bus: u8, device: u8, func: u8) -> alloc::vec::Vec<VirtioCap> {
    let mut caps = alloc::vec::Vec::new();

    // Read the capabilities pointer at offset 0x34. Low byte of the u32.
    let cap_ptr_word = pci::config_read(bus, device, func, CAPABILITIES_POINTER_OFFSET);
    let mut cap_offset = (cap_ptr_word & 0xFF) as u8;

    // Walk the linked list. Capabilities chain via next_ptr; 0 = end.
    while cap_offset != 0 {
        // Word at cap_offset holds bytes:
        //   bit 0-7:   cap_id
        //   bit 8-15:  next_ptr
        //   bit 16-23: length (total cap size including header)
        //   bit 24-31: first byte of body (for vendor-specific: cfg_type)
        let word0 = pci::config_read(bus, device, func, cap_offset);
        let cap_id = (word0 & 0xFF) as u8;
        let next_ptr = ((word0 >> 8) & 0xFF) as u8;
        let _cap_length = ((word0 >> 16) & 0xFF) as u8;

        if cap_id == CAP_VENDOR_SPECIFIC {
            // For vendor-specific caps, byte 3 of the header is the virtio
            // cfg_type. Bytes 4 is the BAR index. Bytes 8-11 are the offset
            // within the BAR. Bytes 12-15 are the region length.
            let cfg_type = ((word0 >> 24) & 0xFF) as u8;

            let word1 = pci::config_read(bus, device, func, cap_offset + 4);
            let bar = (word1 & 0xFF) as u8;

            let word2 = pci::config_read(bus, device, func, cap_offset + 8);
            let region_offset = word2;

            let word3 = pci::config_read(bus, device, func, cap_offset + 12);
            let region_length = word3;

            println!(
                "[virtio]   cap: cfg_type={} ({}), bar={}, offset=0x{:04X}, length=0x{:04X}",
                cfg_type,
                cap_type_name(cfg_type),
                bar,
                region_offset,
                region_length
            );

            caps.push(VirtioCap {
                cfg_type,
                bar,
                offset: region_offset,
                length: region_length,
            });
        }

        cap_offset = next_ptr;
    }

    caps
}
