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
use core::ptr::{read_volatile, write_volatile};

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

// ============================================================
// Common config region field offsets (from the virtio 1.2 spec)
// ============================================================
const COMMON_DEVICE_FEATURE_SELECT: u8 = 0x00;
const COMMON_DEVICE_FEATURE: u8 = 0x04;
const COMMON_DRIVER_FEATURE_SELECT: u8 = 0x08;
const COMMON_DRIVER_FEATURE: u8 = 0x0C;
const COMMON_NUM_QUEUES: u8 = 0x12;
const COMMON_DEVICE_STATUS: u8 = 0x14;
const COMMON_CONFIG_GENERATION: u8 = 0x15;

// ============================================================
// Device status byte bits (the handshake)
// ============================================================
const STATUS_ACKNOWLEDGE: u8 = 0x01;
const STATUS_DRIVER: u8 = 0x02;
const STATUS_DRIVER_OK: u8 = 0x04;
const STATUS_FEATURES_OK: u8 = 0x08;
const STATUS_DEVICE_NEEDS_RESET: u8 = 0x40;
const STATUS_FAILED: u8 = 0x80;

/// virtio feature bits, indexed into the 64-bit feature space.
/// Transport features start at bit 32; device-specific features are 0-31.
const VIRTIO_F_VERSION_1: u64 = 1 << 32;

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

// ============================================================
// VirtioBlk — the device driver state
// ============================================================

/// Driver state for the virtio-blk device.
///
/// Holds the PCI coordinates and the virtual addresses of the four
/// virtio capability regions we care about (common config, notifications,
/// ISR, device config). After construction, methods do the handshake.
pub struct VirtioBlk {
    bus: u8,
    device: u8,
    func: u8,
    /// Virtual address of the common config region (mapped via `map_mmio`).
    common_virt: u64,
}

impl VirtioBlk {
    /// Find the virtio-blk device on the PCI bus, parse its capabilities,
    /// map the common config region into kernel memory, and return a
    /// `VirtioBlk` ready for the handshake.
    ///
    /// Returns `None` if no virtio-blk is attached or it lacks a common
    /// config capability (which would be a spec violation).
    pub fn probe() -> Option<Self> {
        let (bus, device, func) = pci::find_virtio_blk()?;
        let caps = parse_virtio_capabilities(bus, device, func);

        // Find the common config capability
        let common_cap = caps.iter().find(|c| c.cfg_type == VIRTIO_CAP_COMMON)?;
        let common_bar_base = pci::bar_base(bus, device, func, common_cap.bar);
        let common_phys = common_bar_base + common_cap.offset as u64;
        let common_virt = crate::memory::map_mmio(common_phys, common_cap.length as u64);

        println!(
            "[virtio] common config: BAR{}+0x{:X} = phys 0x{:X} = virt 0x{:X}",
            common_cap.bar, common_cap.offset, common_phys, common_virt
        );

        Some(Self {
            bus,
            device,
            func,
            common_virt,
        })
    }

    // ---------- Common config region MMIO access ----------

    /// Read 8 bits from the common config region at `offset`.
    fn read_common_u8(&self, offset: u8) -> u8 {
        // SAFETY: the common config region was mapped via `map_mmio` and
        // points at device registers. Volatile read is required so the
        // compiler doesn't elide or reorder the load.
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u8) }
    }

    /// Write 8 bits to the common config region at `offset`.
    fn write_common_u8(&self, offset: u8, value: u8) {
        // SAFETY: see `read_common_u8`. Writes to device registers have
        // side effects; volatile prevents optimization.
        unsafe {
            write_volatile(
                (self.common_virt + offset as u64) as *mut u8,
                value,
            );
        }
    }

    /// Read 32 bits from the common config region at `offset`.
    fn read_common_u32(&self, offset: u8) -> u32 {
        // SAFETY: as above. 4-byte aligned offset (we only use 0x00, 0x04,
        // 0x08, 0x0C) so no alignment fault.
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u32) }
    }

    /// Write 32 bits to the common config region at `offset`.
    fn write_common_u32(&self, offset: u8, value: u32) {
        unsafe {
            write_volatile(
                (self.common_virt + offset as u64) as *mut u32,
                value,
            );
        }
    }

    // ---------- Handshake helpers ----------

    /// Read the device_status byte from common config.
    pub fn read_status(&self) -> u8 {
        self.read_common_u8(COMMON_DEVICE_STATUS)
    }

    /// Write the device_status byte. Replaces the previous value entirely
    /// (each handshake step writes a fresh value, not OR'd in).
    pub fn write_status(&self, value: u8) {
        self.write_common_u8(COMMON_DEVICE_STATUS, value);
    }

    // ---------- Handshake phases (one method per Lesson 6 step) ----------

    /// Step 6.2: reset, ACKNOWLEDGE, DRIVER.
    ///
    /// - Reset (write 0) clears any prior state. The device returns to fresh.
    /// - ACKNOWLEDGE tells the device "the guest knows about you."
    /// - DRIVER tells the device "the guest has a driver for this kind of
    ///   device" (in our case, virtio-blk).
    ///
    /// After this returns, the device is in the DRIVER state — ready for
    /// feature negotiation (step 6.3) but not yet ready for I/O.
    pub fn reset_and_acknowledge(&self) {
        let before = self.read_status();
        println!("[virtio]   device_status before reset: 0x{:02X}", before);

        // Reset
        self.write_status(0);
        let after_reset = self.read_status();
        println!("[virtio]   device_status after reset:  0x{:02X}", after_reset);
        assert_eq!(
            after_reset, 0,
            "device didn't reset cleanly — hardware may need a moment"
        );

        // ACKNOWLEDGE
        self.write_status(STATUS_ACKNOWLEDGE);
        let after_ack = self.read_status();
        println!("[virtio]   device_status after ACK:    0x{:02X}", after_ack);

        // DRIVER (combined with ACK — the spec accumulates bits)
        self.write_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        let after_driver = self.read_status();
        println!(
            "[virtio]   device_status after DRIVER:   0x{:02X}",
            after_driver
        );
    }

    /// Step 6.3: feature negotiation.
    ///
    /// The device advertises which features it supports via
    /// `device_feature`. The driver picks the subset it can use and
    /// writes that to `driver_feature`. Both halves (low 32, high 32)
    /// must be written via the corresponding `*_select` index. After
    /// writing, the driver sets FEATURES_OK and re-reads the status —
    /// if the device didn't accept the proposed subset, it clears
    /// FEATURES_OK and the driver must retry or give up.
    ///
    /// mini-os currently supports only `VIRTIO_F_VERSION_1` (modern
    /// transport compliance, bit 32 = bit 0 of the high half). Real
    /// Linux claims more (flush, discard, write zeroes) for performance
    /// and correctness.
    pub fn negotiate_features(&self) {
        // Read what the device supports, both halves
        self.write_common_u32(COMMON_DEVICE_FEATURE_SELECT, 0);
        let device_features_low = self.read_common_u32(COMMON_DEVICE_FEATURE);
        self.write_common_u32(COMMON_DEVICE_FEATURE_SELECT, 1);
        let device_features_high = self.read_common_u32(COMMON_DEVICE_FEATURE);

        println!(
            "[virtio]   device features low:  0x{:08X}",
            device_features_low
        );
        println!(
            "[virtio]   device features high: 0x{:08X}",
            device_features_high
        );

        // Compute the subset we support. VIRTIO_F_VERSION_1 is bit 32
        // (bit 0 of the high half). We claim it only if the device
        // advertises it — claiming an unsupported feature would make
        // FEATURES_OK fail.
        let supported_low: u32 = 0;
        let supported_high: u32 = if device_features_high & 0x1 != 0 {
            0x1
        } else {
            0
        };

        println!(
            "[virtio]   driver features low:  0x{:08X}",
            supported_low
        );
        println!(
            "[virtio]   driver features high: 0x{:08X}{}",
            supported_high,
            if supported_high & 0x1 != 0 {
                " (VIRTIO_F_VERSION_1)"
            } else {
                ""
            }
        );

        // Write the negotiated subset
        self.write_common_u32(COMMON_DRIVER_FEATURE_SELECT, 0);
        self.write_common_u32(COMMON_DRIVER_FEATURE, supported_low);
        self.write_common_u32(COMMON_DRIVER_FEATURE_SELECT, 1);
        self.write_common_u32(COMMON_DRIVER_FEATURE, supported_high);

        // Set FEATURES_OK
        self.write_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);
        let status = self.read_status();
        println!(
            "[virtio]   device_status after FEATURES_OK: 0x{:02X}",
            status
        );

        // The spec requires this re-read: if the device doesn't accept
        // our feature subset, it clears FEATURES_OK. We'd have to retry
        // or give up.
        assert!(
            status & STATUS_FEATURES_OK != 0,
            "device rejected our features — FEATURES_OK was cleared"
        );
    }

    /// Step 6.4: DRIVER_OK — the final "go" bit.
    ///
    /// Once DRIVER_OK is set, the device is in operational state. The
    /// device may start using configured virtqueues (Lesson 7 sets those
    /// up). Before this bit, configuring queues is safe — the device
    /// ignores them.
    ///
    /// Reads back the status after writing to check for FAILED or
    /// DEVICE_NEEDS_RESET. If either is set, the device couldn't enter
    /// operational state — we'd have to investigate.
    pub fn set_driver_ok(&self) {
        self.write_status(
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );
        let status = self.read_status();
        println!(
            "[virtio]   device_status after DRIVER_OK: 0x{:02X}",
            status
        );

        assert!(
            status & STATUS_FAILED == 0,
            "device entered FAILED state during DRIVER_OK"
        );
        assert!(
            status & STATUS_DEVICE_NEEDS_RESET == 0,
            "device requested reset during DRIVER_OK — internal error"
        );
        assert!(
            status & STATUS_DRIVER_OK != 0,
            "DRIVER_OK didn't stick — device rejected operational state"
        );
        println!("[virtio] device is live — ready for virtqueue setup");
    }
}
