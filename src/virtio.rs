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

extern crate alloc;

use crate::println;
use crate::pci;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU16, Ordering};

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
const COMMON_QUEUE_SELECT: u8 = 0x16;
const COMMON_QUEUE_SIZE: u8 = 0x18;
const COMMON_QUEUE_MSIX_VECTOR: u8 = 0x1A;
const COMMON_QUEUE_ENABLE: u8 = 0x1C;
const COMMON_QUEUE_NOTIFY_OFF: u8 = 0x1E;
const COMMON_QUEUE_DESC: u8 = 0x20;
const COMMON_QUEUE_AVAIL: u8 = 0x28;
const COMMON_QUEUE_USED: u8 = 0x30;

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
///
/// `notify_off_multiplier` is only meaningful for the notification
/// capability (cfg_type = VIRTIO_CAP_NOTIFY); zero otherwise.
#[derive(Debug, Clone, Copy)]
pub struct VirtioCap {
    pub cfg_type: u8,
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
    pub notify_off_multiplier: u32,
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

            // word4 at offset 16 holds the notify_off_multiplier, only
            // meaningful for the notification capability. For other caps
            // the spec leaves that region undefined; we read it but only
            // use the value when cfg_type == NOTIFY.
            let word4 = pci::config_read(bus, device, func, cap_offset + 16);
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

// ============================================================
// VirtioBlk — the device driver state
// ============================================================

/// Driver state for the virtio-blk device.
///
/// Holds the PCI coordinates and the virtual addresses of the
/// capability regions we use: common config (handshake + queue
/// configuration) and notifications (for kicking the device).
pub struct VirtioBlk {
    bus: u8,
    device: u8,
    func: u8,
    /// Virtual address of the common config region (mapped via `map_mmio`).
    common_virt: u64,
    /// Virtual address of the notification capability region. The kick
    /// path writes the queue index at `notify_virt + queue_notify_off *
    /// notify_off_multiplier` to wake the device.
    notify_virt: u64,
    /// Multiplier (in bytes) for computing the per-queue notify offset.
    /// Read from the notification capability's body.
    notify_off_multiplier: u32,
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

        // Find the notification capability — needed for the kick path.
        let notify_cap = caps.iter().find(|c| c.cfg_type == VIRTIO_CAP_NOTIFY)?;
        let notify_bar_base = pci::bar_base(bus, device, func, notify_cap.bar);
        let notify_phys = notify_bar_base + notify_cap.offset as u64;
        let notify_virt = crate::memory::map_mmio(notify_phys, notify_cap.length as u64);

        println!(
            "[virtio] notifications: BAR{}+0x{:X} = phys 0x{:X} = virt 0x{:X} (multiplier {})",
            notify_cap.bar,
            notify_cap.offset,
            notify_phys,
            notify_virt,
            notify_cap.notify_off_multiplier,
        );

        Some(Self {
            bus,
            device,
            func,
            common_virt,
            notify_virt,
            notify_off_multiplier: notify_cap.notify_off_multiplier,
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

    /// Read 16 bits from the common config region at `offset`.
    fn read_common_u16(&self, offset: u8) -> u16 {
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u16) }
    }

    /// Write 16 bits to the common config region at `offset`.
    fn write_common_u16(&self, offset: u8, value: u16) {
        unsafe {
            write_volatile(
                (self.common_virt + offset as u64) as *mut u16,
                value,
            );
        }
    }

    /// Write 64 bits to the common config region at `offset`. Used for
    /// queue_desc / queue_avail / queue_used — the physical addresses of
    /// the virtqueue regions.
    fn write_common_u64(&self, offset: u8, value: u64) {
        unsafe {
            write_volatile(
                (self.common_virt + offset as u64) as *mut u64,
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

    /// Step 7.2: configure virtqueue 0 (the request queue) in common config.
    ///
    /// Writes the seven queue fields in the order the spec requires:
    ///   1. queue_select = 0  — target the request queue
    ///   2. queue_size = N    — negotiate down from device max to our size
    ///   3. queue_desc        — physical address of descriptor table
    ///   4. queue_avail       — physical address of available ring
    ///   5. queue_used        — physical address of used ring
    ///   6. queue_enable = 1  — "I'm done, activate"
    ///
    /// The device treats `queue_enable` as the commit bit — before that
    /// write it ignores the queue entirely. Reading back `queue_enable`
    /// and seeing 1 confirms acceptance.
    pub fn configure_queue(&self, queue: &VirtQueue) {
        println!("[virtio] configuring virtqueue 0 in common config");

        // 1. Select queue 0 (the request queue for virtio-blk)
        self.write_common_u16(COMMON_QUEUE_SELECT, 0);

        // 2. Negotiate size. Read the device's max, then write back ours.
        let device_max = self.read_common_u16(COMMON_QUEUE_SIZE);
        println!(
            "[virtio]   device max queue_size: {} (we want {})",
            device_max,
            queue.queue_size()
        );
        assert!(
            queue.queue_size() <= device_max,
            "device max queue_size {} < our requested {}",
            device_max,
            queue.queue_size()
        );
        self.write_common_u16(COMMON_QUEUE_SIZE, queue.queue_size());

        // 3-5. Write the three region addresses. The device reads these
        // once at queue_enable time and DMAs from them forever after.
        self.write_common_u64(COMMON_QUEUE_DESC, queue.desc_phys());
        self.write_common_u64(COMMON_QUEUE_AVAIL, queue.avail_phys());
        self.write_common_u64(COMMON_QUEUE_USED, queue.used_phys());

        // 6. Enable. The device checks all the above only at this point.
        self.write_common_u16(COMMON_QUEUE_ENABLE, 1);

        // Verify acceptance — read-back should be 1.
        let enable_readback = self.read_common_u16(COMMON_QUEUE_ENABLE);
        println!(
            "[virtio]   queue_enable readback: {} (expect 1)",
            enable_readback
        );
        assert_eq!(
            enable_readback, 1,
            "device rejected queue configuration — queue_enable didn't stick"
        );
        println!("[virtio] virtqueue 0 active");
    }

    /// Step 7.5: kick — wake the device so it processes new submissions.
    ///
    /// Writes the queue index (0 for virtio-blk's request queue) to the
    /// notification register. The address is computed from the
    /// notification capability base plus the per-queue offset:
    ///
    ///   notify_addr = notify_cap_base + queue_notify_off * multiplier
    ///
    /// `queue_notify_off` is read from common config — it's the device's
    /// way of saying "for this queue, write the kick at this offset."
    /// `multiplier` is a constant from the notification capability body,
    /// usually 0 (meaning "ignore queue_notify_off, just write at base").
    ///
    /// After this store, the device wakes (in QEMU, that's a host-side
    /// thread switch) and processes everything in the available ring.
    pub fn kick(&self) {
        // Per-queue offset for this queue, as advertised by the device.
        let queue_notify_off = self.read_common_u16(COMMON_QUEUE_NOTIFY_OFF) as u64;
        let off = queue_notify_off * (self.notify_off_multiplier as u64);
        let notify_addr = self.notify_virt + off;

        // SAFETY: notify_virt was mapped via map_mmio in probe().
        // Writing the queue index (0) is the spec-defined kick action.
        // Volatile so the compiler can't elide the store.
        unsafe {
            write_volatile(notify_addr as *mut u16, 0);
        }

        println!(
            "[virtio] kick: wrote queue 0 to notify register at virt 0x{:X} (notify_off={} * mult={})",
            notify_addr, queue_notify_off, self.notify_off_multiplier
        );
    }
}

// ============================================================
// VirtQueue — the shared-memory ring buffer (Lesson 7)
// ============================================================

/// Bytes occupied by one descriptor entry: addr(8) + len(4) + flags(2) + next(2).
const VIRTQ_DESC_SIZE: usize = 16;

// Descriptor flags (bits in the `flags` field).
const VIRTQ_DESC_F_NEXT: u16 = 0x01;
const VIRTQ_DESC_F_WRITE: u16 = 0x02;

/// Layout of one descriptor entry. `repr(C)` guarantees the fields land
/// at the spec-mandated offsets: addr at 0, len at 8, flags at 12, next at 14.
#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

// ============================================================
// virtio-blk request format (Lesson 8)
// ============================================================

/// virtio-blk operation types — the `type` field of `VirtioBlkReq`.
const VIRTIO_BLK_T_IN: u32 = 0;    // read
const VIRTIO_BLK_T_OUT: u32 = 1;   // write
const VIRTIO_BLK_T_FLUSH: u32 = 4; // flush cache

/// virtio-blk status byte values — written by the device on completion.
const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_IOERR: u8 = 1;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

/// virtio-blk request header. Sent as the first buffer in every request.
/// `repr(C)` so fields land at the spec-mandated offsets.
///
/// After this struct, the chain continues with the data buffer (for the
/// bytes being read or written) and a one-byte status field. The device
/// reads the header, performs the operation, fills the data buffer (for
/// reads), and writes the status byte.
#[repr(C)]
#[derive(Debug)]
pub struct VirtioBlkReq {
    /// Operation code (VIRTIO_BLK_T_IN, _OUT, _FLUSH).
    pub type_: u32,
    /// Reserved by the spec; driver must zero, device ignores.
    pub reserved: u32,
    /// Starting sector number (sectors are always 512 B for virtio-blk).
    pub sector: u64,
}

impl VirtioBlkReq {
    pub fn new_read(sector: u64) -> Self {
        Self {
            type_: VIRTIO_BLK_T_IN,
            reserved: 0,
            sector,
        }
    }
}

/// A split virtqueue: three regions in guest memory that the kernel and
/// the device use to exchange requests.
///
/// Each region is allocated as a single 4 KiB frame from the frame allocator.
/// That gives us page alignment and physical contiguity for free, both of
/// which the device requires. The frame is reachable from kernel code via
/// the bootloader's direct map (`phys_offset() + phys`).
///
/// For `queue_size <= 64`, each region fits in one frame:
///   - descriptor table: 64 × 16 = 1024 B
///   - available ring:    2 + 2 + (64 × 2) + 2 = 132 B
///   - used ring:         2 + 2 + (64 × 8) + 2 = 518 B
pub struct VirtQueue {
    queue_size: u16,

    // Frame numbers from the frame allocator. Kept so we can free them
    // when VirtQueue is dropped (cleanup not implemented yet — kernel
    // lives forever, queues live as long as the kernel does).
    desc_frame: u64,
    avail_frame: u64,
    used_frame: u64,

    // Virtual addresses — kernel-side access via the direct map.
    desc_virt: u64,
    avail_virt: u64,
    used_virt: u64,

    // Physical addresses — what we write into common config so the
    // device knows where to DMA.
    desc_phys: u64,
    avail_phys: u64,
    used_phys: u64,

    // Free descriptor indices. Treated as a stack — pop on alloc,
    // push on free. O(1) both ways, no fragmentation.
    free_list: alloc::vec::Vec<u16>,

    /// Last `used->idx` we processed. Initialized to 0 (no completions
    /// seen yet). Used by `poll_completion` to detect new entries.
    last_used_idx: u16,
}

impl VirtQueue {
    /// Allocate the three regions for a queue of `queue_size` entries.
    /// Each region gets its own 4 KiB frame, zeroed so the device starts
    /// from a clean state.
    pub fn new(queue_size: u16) -> Self {
        let q = queue_size as usize;
        let desc_bytes = q * VIRTQ_DESC_SIZE;
        let avail_bytes = 6 + q * 2;  // flags + idx + ring + used_event
        let used_bytes = 6 + q * 8;   // flags + idx + ring entries + avail_event

        // Assert our single-frame-per-region assumption. Larger queues
        // would need multi-frame allocations; we don't need that yet.
        assert!(desc_bytes <= 4096, "descriptor table doesn't fit in one frame");
        assert!(avail_bytes <= 4096, "available ring doesn't fit in one frame");
        assert!(used_bytes <= 4096, "used ring doesn't fit in one frame");

        let desc_frame = crate::frame_allocator::allocate()
            .expect("out of frames for descriptor table");
        let avail_frame = crate::frame_allocator::allocate()
            .expect("out of frames for available ring");
        let used_frame = crate::frame_allocator::allocate()
            .expect("out of frames for used ring");

        let offset = crate::memory::phys_offset();
        let desc_phys = desc_frame * 4096;
        let avail_phys = avail_frame * 4096;
        let used_phys = used_frame * 4096;
        let desc_virt = desc_phys + offset;
        let avail_virt = avail_phys + offset;
        let used_virt = used_phys + offset;

        // Zero the regions. The spec assumes zeroed state — non-zero
        // garbage in `avail->idx` or `used->idx` would desync the
        // kernel and device from the start.
        // SAFETY: frames are freshly allocated and reachable via the
        // direct map. No aliases.
        unsafe {
            core::ptr::write_bytes(desc_virt as *mut u8, 0, 4096);
            core::ptr::write_bytes(avail_virt as *mut u8, 0, 4096);
            core::ptr::write_bytes(used_virt as *mut u8, 0, 4096);
        }

        println!("[virtio] virtqueue allocated (queue_size={})", queue_size);
        println!(
            "[virtio]   descriptor table: virt 0x{:X} phys 0x{:X} ({} bytes used / 4096)",
            desc_virt, desc_phys, desc_bytes
        );
        println!(
            "[virtio]   available ring:   virt 0x{:X} phys 0x{:X} ({} bytes used / 4096)",
            avail_virt, avail_phys, avail_bytes
        );
        println!(
            "[virtio]   used ring:        virt 0x{:X} phys 0x{:X} ({} bytes used / 4096)",
            used_virt, used_phys, used_bytes
        );

        Self {
            queue_size,
            desc_frame,
            avail_frame,
            used_frame,
            desc_virt,
            avail_virt,
            used_virt,
            desc_phys,
            avail_phys,
            used_phys,
            free_list: (0..queue_size).collect(),
            last_used_idx: 0,
        }
    }

    pub fn queue_size(&self) -> u16 {
        self.queue_size
    }

    pub fn desc_phys(&self) -> u64 {
        self.desc_phys
    }

    pub fn avail_phys(&self) -> u64 {
        self.avail_phys
    }

    pub fn used_phys(&self) -> u64 {
        self.used_phys
    }

    /// Pop a free descriptor index from the free list. Returns `None`
    /// if all `queue_size` descriptors are in use.
    pub fn alloc_descriptor(&mut self) -> Option<u16> {
        self.free_list.pop()
    }

    /// Push a descriptor index back onto the free list. Caller's
    /// responsibility to ensure no device is still reading the
    /// descriptor (typically called only after seeing the completion
    /// in the used ring).
    pub fn free_descriptor(&mut self, idx: u16) {
        debug_assert!(
            idx < self.queue_size,
            "descriptor index {} out of range (queue_size {})",
            idx,
            self.queue_size
        );
        self.free_list.push(idx);
    }

    /// Number of descriptors currently free.
    pub fn num_free(&self) -> usize {
        self.free_list.len()
    }

    /// Write the four fields of descriptor `idx`. Used by future steps
    /// to build a descriptor chain before submitting it.
    ///
    /// SAFETY-conscious: writes via volatile so the compiler can't
    /// reorder or coalesce with device reads.
    pub fn write_descriptor(
        &self,
        idx: u16,
        addr: u64,
        len: u32,
        flags: u16,
        next: u16,
    ) {
        let entry_virt = self.desc_virt + (idx as u64) * VIRTQ_DESC_SIZE as u64;
        // SAFETY: `idx` is in range (caller contract), `entry_virt`
        // points at one descriptor inside the descriptor-table frame,
        // which we own and which is reachable via the direct map.
        unsafe {
            let ptr = entry_virt as *mut VirtqDesc;
            write_volatile(&mut (*ptr).addr, addr);
            write_volatile(&mut (*ptr).len, len);
            write_volatile(&mut (*ptr).flags, flags);
            write_volatile(&mut (*ptr).next, next);
        }
    }

    /// Publish a chain head to the available ring and advance `idx`.
    ///
    /// The spec requires a write memory barrier between filling in the
    /// descriptor data and bumping `idx` — otherwise the CPU may make
    /// the new `idx` visible to the device *before* the descriptor
    /// fields are visible, and the device would DMA from a half-written
    /// descriptor.
    ///
    /// We achieve this with `AtomicU16::fetch_add(1, Ordering::Release)`
    /// on `avail->idx`. Release ordering inserts the barrier for us.
    ///
    /// Returns the old `idx` value (i.e., the slot we just wrote into).
    pub fn submit_chain(&self, head_index: u16) -> u16 {
        let q = self.queue_size as u64;

        // Pointer to avail->idx (offset 2 from the region base).
        // SAFETY: avail_virt is the kernel-side address of a freshly
        // allocated frame; +2 lands on the idx field inside it.
        let idx_atomic: *const AtomicU16 =
            (self.avail_virt + 2) as *const AtomicU16;

        // Read current idx. Relaxed is fine here — no other publication
        // depends on this read.
        let old_idx = unsafe { (*idx_atomic).load(Ordering::Relaxed) };

        // Write the head descriptor index into ring[old_idx % N].
        // Offset 4 (past flags + idx) + slot * 2 bytes.
        let slot = (old_idx as u64) % q;
        let ring_entry_virt = self.avail_virt + 4 + slot * 2;
        unsafe {
            write_volatile(ring_entry_virt as *mut u16, head_index);
        }

        // Advance idx with Release ordering. This is the memory barrier:
        // all prior writes (the ring entry AND the descriptor fields the
        // caller filled in) become globally visible before the new idx
        // does. The device, polling idx, will see the new value only
        // after the data is on record.
        let _new_idx = unsafe { (*idx_atomic).fetch_add(1, Ordering::Release) };

        println!(
            "[virtio] submit_chain({}): avail->idx {} -> {}",
            head_index,
            old_idx,
            old_idx.wrapping_add(1)
        );

        old_idx
    }

    /// Poll the used ring for new completions. Returns `Some((id, len))`
    /// if a new completion has arrived since the last call, `None` if no
    /// new completions.
    ///
    /// - `id` is the head descriptor index the kernel originally
    ///   submitted — use it to find and free the chain.
    /// - `len` is the number of bytes the device actually wrote to the
    ///   data buffer. For a read: the bytes read. For a write: 0.
    ///
    /// Reads `used->idx` with Acquire ordering. This pairs with the
    /// device's Release-ordered store: we won't see the new idx until
    /// the device's prior writes (the used ring entry fields) are also
    /// visible to us. So reading the entry right after seeing idx
    /// advance is safe.
    pub fn poll_completion(&mut self) -> Option<(u32, u32)> {
        let q = self.queue_size as u64;

        // Pointer to used->idx (offset 2 from region base).
        let idx_atomic: *const AtomicU16 =
            (self.used_virt + 2) as *const AtomicU16;

        // Acquire load pairs with the device's Release store.
        let current = unsafe { (*idx_atomic).load(Ordering::Acquire) };

        if current == self.last_used_idx {
            return None;
        }

        // Read the entry at last_used_idx % N. Layout: offset 4 (past
        // flags+idx), each entry is 8 bytes (id u32, len u32).
        let slot = (self.last_used_idx as u64) % q;
        let entry_virt = self.used_virt + 4 + slot * 8;
        let (id, len) = unsafe {
            (
                read_volatile(entry_virt as *const u32),
                read_volatile((entry_virt + 4) as *const u32),
            )
        };

        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        println!(
            "[virtio] poll_completion: used->idx {} -> {}, id={} len={}",
            self.last_used_idx.wrapping_sub(1),
            current,
            id,
            len
        );

        Some((id, len))
    }
}
