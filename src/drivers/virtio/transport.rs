use super::*;

// Offsets are relative to the start of a PCI capability.
const VIRTIO_CAP_BAR: u8 = 4;
const VIRTIO_CAP_REGION_OFFSET: u8 = 8;
const VIRTIO_CAP_REGION_LENGTH: u8 = 12;
const VIRTIO_CAP_NOTIFY_MULTIPLIER: u8 = 16;
const MSIX_TABLE_LOCATION: u8 = 4;
const MSIX_TABLE_SIZE_MASK: u16 = 0x07ff;
const MSIX_FUNCTION_MASK: u16 = 1 << 14;
const MSIX_TABLE_BAR_MASK: u32 = 0b111;
const MSIX_TABLE_OFFSET_MASK: u32 = !MSIX_TABLE_BAR_MASK;
const MSIX_TABLE_ENTRY_BYTES: u64 = 16;
const CAP_ID_AND_NEXT_MASK: u32 = 0x0000_ffff;
const CAP_CONTROL_SHIFT: u32 = 16;

/// One parsed virtio vendor-specific capability.
///
/// Describes a register region by type, BAR index, byte offset and length.
/// The driver translates (bar, offset) into a virtual
/// address (via the BAR's base address plus the kernel's MMIO mapping)
/// and accesses device registers using volatile memory loads/stores.
///
/// `notify_off_multiplier` is only meaningful for the notification
/// capability (cfg_type = VIRTIO_CAP_NOTIFY); other types retain an unused
/// value read at the same offset. The parser does not validate cap lengths.
#[derive(Debug, Clone, Copy)]
pub struct VirtioCap {
    pub cfg_type: u8,
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
    pub notify_off_multiplier: u32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MsixCap {
    pub(super) table_virt: u64,
    pub(super) entries: u16,
}

pub(super) fn parse_msix_capability(bus: u8, device: u8, func: u8) -> Option<MsixCap> {
    let mut offset =
        (pci::config_read(bus, device, func, CAPABILITIES_POINTER_OFFSET) & 0xff) as u8;
    while offset != 0 {
        let header = pci::config_read(bus, device, func, offset);
        let next = ((header >> 8) & 0xff) as u8;
        if (header & 0xff) as u8 == CAP_MSIX {
            let control = (header >> CAP_CONTROL_SHIFT) as u16;
            // The field stores the highest entry index, so add one for the count.
            let entries = (control & MSIX_TABLE_SIZE_MASK) + 1;
            let table = pci::config_read(bus, device, func, offset + MSIX_TABLE_LOCATION);
            let table_bar = (table & MSIX_TABLE_BAR_MASK) as u8;
            let table_phys = pci::bar_base(bus, device, func, table_bar)
                + (table & MSIX_TABLE_OFFSET_MASK) as u64;
            let table_virt =
                crate::memory::map_mmio(table_phys, entries as u64 * MSIX_TABLE_ENTRY_BYTES);
            // Message Control shares its dword with cap ID/next. Preserve them.
            // Enable MSI-X and clear the function-wide interrupt mask.
            let enabled_control = (control | MSIX_ENABLE) & !MSIX_FUNCTION_MASK;
            let enabled_header =
                (header & CAP_ID_AND_NEXT_MASK) | ((enabled_control as u32) << CAP_CONTROL_SHIFT);
            pci::config_write(bus, device, func, offset, enabled_header);
            println!(
                "[virtio] MSI-X table: {} entries at phys 0x{:x}",
                entries, table_phys
            );
            return Some(MsixCap {
                table_virt,
                entries,
            });
        }
        offset = next;
    }
    None
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
/// Assumes a well-formed list: malformed cycles and lengths are unchecked.
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
        //   bit 16-23: vendor-specific capability length
        //   bit 24-31: first byte of body (for vendor-specific: cfg_type)
        let word0 = pci::config_read(bus, device, func, cap_offset);
        let cap_id = (word0 & 0xFF) as u8;
        let next_ptr = ((word0 >> 8) & 0xFF) as u8;
        let _cap_length = ((word0 >> 16) & 0xFF) as u8;

        if cap_id == CAP_VENDOR_SPECIFIC {
            // For vendor-specific caps, byte 3 of the header is the virtio
            // cfg_type. Byte 4 is the BAR index. Bytes 8-11 are the offset
            // within the BAR. Bytes 12-15 are the region length.
            let cfg_type = ((word0 >> 24) & 0xFF) as u8;

            let word1 = pci::config_read(bus, device, func, cap_offset + VIRTIO_CAP_BAR);
            let bar = (word1 & 0xFF) as u8;

            let word2 = pci::config_read(bus, device, func, cap_offset + VIRTIO_CAP_REGION_OFFSET);
            let region_offset = word2;

            let word3 = pci::config_read(bus, device, func, cap_offset + VIRTIO_CAP_REGION_LENGTH);
            let region_length = word3;

            // word4 at offset 16 holds the notify_off_multiplier, only
            // meaningful for the notification capability. For other caps
            // the spec leaves that region undefined; we read it but only
            // use the value when cfg_type == NOTIFY.
            let word4 =
                pci::config_read(bus, device, func, cap_offset + VIRTIO_CAP_NOTIFY_MULTIPLIER);
            let notify_off_multiplier = word4;

            if cfg_type == VIRTIO_CAP_NOTIFY {
                println!(
                    "[virtio]   cap: cfg_type={} ({}), bar={}, offset=0x{:04X}, length=0x{:04X}, notify_off_multiplier={}",
                    cfg_type,
                    cap_type_name(cfg_type),
                    bar,
                    region_offset,
                    region_length,
                    notify_off_multiplier,
                );
            } else {
                println!(
                    "[virtio]   cap: cfg_type={} ({}), bar={}, offset=0x{:04X}, length=0x{:04X}",
                    cfg_type,
                    cap_type_name(cfg_type),
                    bar,
                    region_offset,
                    region_length,
                );
            }

            caps.push(VirtioCap {
                cfg_type,
                bar,
                offset: region_offset,
                length: region_length,
                notify_off_multiplier,
            });
        }

        cap_offset = next_ptr;
    }

    caps
}
