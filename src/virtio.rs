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

use crate::net;
use crate::net::arp::{self, ArpPacket, ARP_PACKET_LEN, OPER_REQUEST};
use crate::net::ethernet::{EthernetHeader, ETHERNET_HEADER_LEN, ETHERTYPE_ARP, ETHERTYPE_IPV4};
use crate::net::ipv4::{Ipv4Header, IPV4_HEADER_LEN};
use crate::net::udp::{self, UdpDatagram, UdpHeader, UDP_HEADER_LEN};
use crate::pci;
use crate::println;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

static FIRST_DISK_COMPLETION_REPORTED: AtomicBool = AtomicBool::new(false);
static FIRST_NET_WAKEUP_REPORTED: AtomicBool = AtomicBool::new(false);
static FIRST_NET_TX_COMPLETION_REPORTED: AtomicBool = AtomicBool::new(false);

/// Offset of the capabilities pointer in PCI config space.
const CAPABILITIES_POINTER_OFFSET: u8 = 0x34;

/// Capability ID for "vendor specific" — virtio uses this type.
const CAP_VENDOR_SPECIFIC: u8 = 0x09;
const CAP_MSIX: u8 = 0x11;
const MSIX_ENABLE: u16 = 1 << 15;

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

/// virtio-net feature bit: device config contains a valid MAC address.
const VIRTIO_NET_F_MAC: u32 = 1 << 5;

/// RX buffers are large enough for a virtio-net header plus one Ethernet MTU.
const VIRTIO_NET_RX_BUFFER_LEN: u32 = 2048;

/// Initial receive-buffer supply for Lesson 20.
const VIRTIO_NET_RX_BUFFER_COUNT: usize = 8;

/// virtio-net prepends this fixed header to every TX/RX frame.
const VIRTIO_NET_HDR_LEN: usize = 12;

/// Payload bytes a raw Ethernet frame can carry without jumbo frames.
const ETHERNET_MTU: usize = 1500;

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

#[derive(Debug, Clone, Copy)]
struct MsixCap {
    table_virt: u64,
    entries: u16,
}

fn parse_msix_capability(bus: u8, device: u8, func: u8) -> Option<MsixCap> {
    let mut offset =
        (pci::config_read(bus, device, func, CAPABILITIES_POINTER_OFFSET) & 0xff) as u8;
    while offset != 0 {
        let header = pci::config_read(bus, device, func, offset);
        let next = ((header >> 8) & 0xff) as u8;
        if (header & 0xff) as u8 == CAP_MSIX {
            let control = (header >> 16) as u16;
            let entries = (control & 0x7ff) + 1;
            let table = pci::config_read(bus, device, func, offset + 4);
            let bir = (table & 7) as u8;
            let table_phys = pci::bar_base(bus, device, func, bir) + (table & !7) as u64;
            let table_virt = crate::memory::map_mmio(table_phys, entries as u64 * 16);
            // Message Control shares its dword with cap ID/next. Preserve them.
            let enabled_control = (control | MSIX_ENABLE) & !(1 << 14);
            let enabled_header = (header & 0x0000_ffff) | ((enabled_control as u32) << 16);
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
    /// The virtqueue, owned by the device after `setup_queue` runs.
    /// `read_sector` and future I/O methods unwrap this.
    queue: Option<VirtQueue>,
    msix: MsixCap,
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
        let msix = parse_msix_capability(bus, device, func)?;

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
            queue: None,
            msix,
        })
    }

    /// Step 9.1: allocate and configure the virtqueue, stashing it inside
    /// the device handle. After this call, `read_sector` (and future I/O
    /// methods) can use `self.queue.as_mut().unwrap()`.
    ///
    /// This wraps what used to be the two-step `VirtQueue::new` +
    /// `configure_queue` dance in main.rs. Callers don't need to know
    /// the queue exists.
    pub fn setup_queue(&mut self, queue_size: u16) {
        let queue = VirtQueue::new(queue_size);
        self.configure_queue_at(0, &queue, "virtio-blk request");
        self.configure_queue_interrupt();
        self.queue = Some(queue);
    }

    fn configure_queue_interrupt(&self) {
        assert!(self.msix.entries > 0);
        let entry = self.msix.table_virt;
        unsafe {
            write_volatile(entry as *mut u32, crate::apic::MSI_ADDRESS);
            write_volatile((entry + 4) as *mut u32, 0);
            write_volatile(
                (entry + 8) as *mut u32,
                crate::interrupts::VIRTIO_BLK_VECTOR as u32,
            );
            write_volatile((entry + 12) as *mut u32, 0);
        }
        self.write_common_u16(COMMON_QUEUE_SELECT, 0);
        self.write_common_u16(COMMON_QUEUE_MSIX_VECTOR, 0);
        let assigned = self.read_common_u16(COMMON_QUEUE_MSIX_VECTOR);
        assert_eq!(assigned, 0, "virtio-blk rejected MSI-X vector 0");
        println!("[virtio] MSI-X queue 0 -> table 0 -> vector 0x21");
    }

    /// Borrow the virtqueue mutably. Panics if `setup_queue` hasn't run.
    /// Public so test code in main.rs can still poke the queue directly
    /// when verifying lower-level primitives.
    pub fn queue_mut(&mut self) -> &mut VirtQueue {
        self.queue
            .as_mut()
            .expect("queue not set up — call setup_queue first")
    }

    /// Step 9.2: read one 512-byte sector from the disk. Synchronous —
    /// returns when the device responds. Allocates DMA buffers per call
    /// (~12 KB leaked; future optimization: buffer pool).
    ///
    /// The full dance, hidden from callers:
    ///   1. Allocate header/data/status DMA buffers (BlkBuffers)
    ///   2. Write the request header (type=IN, sector)
    ///   3. Build the 3-descriptor chain
    ///   4. Submit the chain (avail->idx advances with Release)
    ///   5. Kick the device
    ///   6. Poll for completion (Acquire on used->idx)
    ///   7. Assert status == OK
    ///   8. Free the descriptors
    ///   9. Copy the 512 bytes out of the DMA buffer into a stack array
    pub fn read_sector(&mut self, sector: u64) -> [u8; 512] {
        println!("[virtio] read_sector({})", sector);
        let buffers = BlkBuffers::new();
        let req = VirtioBlkReq::new_read(sector);
        buffers.write_header(&req);

        let (head, mid, tail) = self
            .queue
            .as_mut()
            .expect("queue not set up")
            .build_read_chain(buffers.header_phys, buffers.data_phys, buffers.status_phys);

        self.queue.as_mut().unwrap().submit_chain(head);
        self.kick();

        let completion = loop {
            // Prevent an interrupt from landing after the ring check and
            // before HLT. A pending MSI-X (or timer tick) is delivered by
            // the atomic STI; HLT sequence in enable_and_hlt().
            x86_64::instructions::interrupts::disable();
            if let Some(completion) = self.queue.as_mut().unwrap().poll_completion() {
                x86_64::instructions::interrupts::enable();
                if crate::interrupts::VIRTIO_BLK_INTERRUPTS.load(Ordering::Acquire) > 0
                    && FIRST_DISK_COMPLETION_REPORTED
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                {
                    println!("[virtio] first disk completion delivered by MSI-X");
                }
                break completion;
            }
            x86_64::instructions::interrupts::enable_and_hlt();
        };

        let status = buffers.read_status();
        assert_eq!(
            status, VIRTIO_BLK_S_OK,
            "read_sector(sector={}) failed: device status={}, completion id={} len={}",
            sector, status, completion.0, completion.1
        );

        let queue = self.queue.as_mut().unwrap();
        queue.free_descriptor(head);
        queue.free_descriptor(mid);
        queue.free_descriptor(tail);

        let mut block = [0u8; 512];
        let data_view = buffers.read_data_prefix(512);
        block.copy_from_slice(&data_view);
        block
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
            write_volatile((self.common_virt + offset as u64) as *mut u8, value);
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
            write_volatile((self.common_virt + offset as u64) as *mut u32, value);
        }
    }

    /// Read 16 bits from the common config region at `offset`.
    fn read_common_u16(&self, offset: u8) -> u16 {
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u16) }
    }

    /// Write 16 bits to the common config region at `offset`.
    fn write_common_u16(&self, offset: u8, value: u16) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u16, value);
        }
    }

    /// Write 64 bits to the common config region at `offset`. Used for
    /// queue_desc / queue_avail / queue_used — the physical addresses of
    /// the virtqueue regions.
    fn write_common_u64(&self, offset: u8, value: u64) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u64, value);
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
        println!(
            "[virtio]   device_status after reset:  0x{:02X}",
            after_reset
        );
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

        println!("[virtio]   driver features low:  0x{:08X}", supported_low);
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
        println!("[virtio]   device_status after DRIVER_OK: 0x{:02X}", status);

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
    pub fn configure_queue_at(&self, queue_index: u16, queue: &VirtQueue, label: &str) {
        println!(
            "[virtio] configuring virtqueue {} ({}) in common config",
            queue_index, label
        );

        // 1. Select the target queue.
        self.write_common_u16(COMMON_QUEUE_SELECT, queue_index);

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
        println!("[virtio] virtqueue {} active", queue_index);
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

        // println!(
        //     "[virtio] kick: wrote queue 0 to notify register at virt 0x{:X} (notify_off={} * mult={})",
        //     notify_addr, queue_notify_off, self.notify_off_multiplier
        // );
    }
}

// ============================================================
// VirtioNet — Lesson 20 network device initialization
// ============================================================

/// One receive buffer posted to the virtio-net RX queue.
pub struct RxBuffer {
    pub desc_idx: u16,
    pub virt: u64,
    pub phys: u64,
    pub len: u32,
    frame: u64,
}

/// One Ethernet frame returned by the device-facing network API.
pub struct Frame {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ethertype: u16,
    pub payload: [u8; ETHERNET_MTU],
    pub payload_len: usize,
}

impl Frame {
    fn from_header_and_payload(
        header: EthernetHeader,
        payload_src: u64,
        payload_len: usize,
    ) -> Self {
        assert!(
            payload_len <= ETHERNET_MTU,
            "Ethernet payload {} exceeds MTU {}",
            payload_len,
            ETHERNET_MTU
        );

        let mut payload = [0u8; ETHERNET_MTU];
        unsafe {
            for (i, byte) in payload.iter_mut().take(payload_len).enumerate() {
                *byte = read_volatile((payload_src + i as u64) as *const u8);
            }
        }

        Self {
            dst: header.dst,
            src: header.src,
            ethertype: header.ethertype,
            payload,
            payload_len,
        }
    }

    pub fn print_payload_prefix(&self, max_len: usize) {
        let n = core::cmp::min(max_len, self.payload_len);
        let mut offset = 0;
        while offset < n {
            crate::print!("[net]   {:02x}: ", offset);
            let line_end = core::cmp::min(offset + 16, n);
            for byte in &self.payload[offset..line_end] {
                crate::print!("{:02x} ", byte);
            }
            crate::println!();
            offset = line_end;
        }
    }
}

/// Driver-facing network device surface.
pub struct NetDevice {
    inner: VirtioNet,
}

/// One packet-level event produced by the network RX dispatcher.
pub enum NetRxEvent {
    Arp(ArpPacket),
    Udp(UdpPacket),
    Tcp,
    Ipv4(Ipv4Packet),
    Other,
}

/// An IPv4 header paired with the exact payload bytes that followed it.
///
/// The RX buffer is recycled as soon as a frame is dispatched. Copying the
/// bounded IPv4 payload here gives higher protocols stable bytes to parse.
pub struct Ipv4Packet {
    pub header: Ipv4Header,
    payload: [u8; ETHERNET_MTU - IPV4_HEADER_LEN],
    payload_len: usize,
}

impl Ipv4Packet {
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len]
    }
}

/// A checksum-verified UDP datagram with its IPv4 endpoints.
pub struct UdpPacket {
    pub ip_header: Ipv4Header,
    pub header: UdpHeader,
    payload: [u8; ETHERNET_MTU - IPV4_HEADER_LEN - UDP_HEADER_LEN],
    payload_len: usize,
}

impl UdpPacket {
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len]
    }
}

lazy_static::lazy_static! {
    static ref NET_DEVICE: spin::Mutex<Option<NetDevice>> = spin::Mutex::new(None);
}

pub fn install_net_device(device: NetDevice) {
    let mac = device.mac();
    *NET_DEVICE.lock() = Some(device);
    println!(
        "[net] device installed for syscall-triggered tests mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
}

pub fn with_net_device<R>(f: impl FnOnce(&mut NetDevice) -> R) -> Option<R> {
    NET_DEVICE.lock().as_mut().map(f)
}

/// Non-spinning variant of [`with_net_device`]: returns `None` if the
/// device is currently owned by another task (a sender mid-transmit).
///
/// The network task uses this because it runs its drain with interrupts
/// disabled (so the empty-check and the block are atomic). A spinning
/// lock there would be fatal: the timer couldn't preempt the spin, the
/// task holding the device would never get the CPU back, and the system
/// would deadlock. Every lock acquisition that *can* spin must happen
/// with interrupts enabled so the timer can always break the cycle.
pub fn try_with_net_device<R>(f: impl FnOnce(&mut NetDevice) -> R) -> Option<R> {
    NET_DEVICE
        .try_lock()
        .and_then(|mut guard| guard.as_mut().map(f))
}

/// PID of the deferred network task, registered at spawn time. The RX
/// interrupt reads this to know which task to wake. `u64::MAX` means "no
/// network task exists" — early boot, or test modes where a syscall
/// drains the RX ring itself. The ISR handles that by skipping the wake.
static NET_TASK_PID: AtomicU64 = AtomicU64::new(u64::MAX);

pub fn register_net_task(pid: crate::process::Pid) {
    NET_TASK_PID.store(pid.0, Ordering::Release);
}

pub fn net_task_pid() -> Option<crate::process::Pid> {
    let raw = NET_TASK_PID.load(Ordering::Acquire);
    if raw == u64::MAX {
        None
    } else {
        Some(crate::process::Pid(raw))
    }
}

/// Deferred network task — Lesson 29's replacement for `network_worker`.
///
/// The old worker looped forever: poll the RX ring, yield, repeat. It
/// burned a schedule slice on every pass even when no frames existed.
/// It drains the RX ring until empty, marks itself Blocked, and sleeps.
/// The RX interrupt can wake it directly; the timer also wakes it as a
/// bounded-latency backstop while virtio-net MSI-X remains unreliable in
/// the current QEMU machine configuration.
///
/// Two rules keep this race-free:
///
/// 1. The drain and the block run with interrupts disabled, so the RX
///    ISR can't fire in between. A frame arriving mid-drain is seen by
///    the drain loop. A frame arriving after the drain pends its MSI in
///    the APIC; the interrupt fires as soon as interrupts re-enable, and
///    `wake` finds the task already Blocked. No lost wakeup.
///
/// 2. The device lock is taken with `try_lock`, never the spinning
///    `lock`. A spinning wait with interrupts disabled would deadlock:
///    the timer couldn't preempt, and whoever holds the device (a task
///    mid-`sendto`) would never run again. On contention the task stays
///    awake and retries next schedule — sends are short, so the retry
///    lands almost immediately.
///
/// This is Linux's NAPI shape: the hardirq handler (our ISR) only
/// schedules the poll; the poll (this task) runs in a context where
/// printing, locking, and transmitting are all safe.
pub unsafe extern "C" fn network_task() {
    println!("[net] deferred RX task started");
    loop {
        x86_64::instructions::interrupts::without_interrupts(|| {
            if try_with_net_device(|device| {
                while device.poll_rx().is_some() {}
                if let Some(outbound) = crate::net::tcp::poll_retransmit(
                    crate::interrupts::TICKS.load(Ordering::Relaxed),
                ) {
                    let _ = device.send_ipv4_packet(
                        crate::net::ipv4::PROTOCOL_TCP,
                        outbound.dst_ip,
                        &outbound.bytes,
                    );
                }
            })
            .is_some()
            {
                crate::task::block_current();
            }
        });
        report_first_net_wakeup();
        crate::task::yield_now();
    }
}

/// One-time serial report when the deferred task observes a genuine RX MSI.
fn report_first_net_wakeup() {
    if crate::interrupts::VIRTIO_NET_RX_INTERRUPTS.load(Ordering::Acquire) > 0
        && FIRST_NET_WAKEUP_REPORTED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        println!("[net] first RX wakeup delivered by MSI-X interrupt");
    }
}

impl NetDevice {
    pub fn from_virtio_net(inner: VirtioNet) -> Self {
        Self { inner }
    }

    pub fn mac(&self) -> [u8; 6] {
        self.inner.mac
    }

    pub fn send_frame(&mut self, eth_frame: &[u8]) {
        // QEMU's virtio-net backend expects the full 64-byte minimum on
        // TX even though physical NICs normally append the 4-byte FCS.
        // The extra zero bytes remain Ethernet padding outside IPv4.
        const ETHERNET_MIN_TX_BUFFER: usize = 64;
        assert!(
            eth_frame.len() >= ETHERNET_HEADER_LEN,
            "send_frame needs at least an Ethernet header"
        );
        assert!(
            eth_frame.len() <= ETHERNET_HEADER_LEN + ETHERNET_MTU,
            "send_frame length {} exceeds Ethernet MTU",
            eth_frame.len()
        );

        // Zero padding sits after the protocol payload and is excluded
        // from IPv4/UDP length fields.
        let tx_len = core::cmp::max(eth_frame.len(), ETHERNET_MIN_TX_BUFFER);
        let buffers = NetTxBuffers::new(eth_frame);
        let (head, tail) = {
            let tx_queue = self
                .inner
                .tx_queue
                .as_mut()
                .expect("virtio-net TX queue not configured");
            let (head, tail) = tx_queue.build_tx_chain(
                buffers.header_phys,
                VIRTIO_NET_HDR_LEN as u32,
                buffers.frame_phys,
                tx_len as u32,
            );
            tx_queue.submit_chain(head);
            (head, tail)
        };

        self.inner.kick_queue(1);

        let interrupts_were_enabled = x86_64::instructions::interrupts::are_enabled();
        let completion = loop {
            x86_64::instructions::interrupts::disable();
            if let Some(completion) = self
                .inner
                .tx_queue
                .as_mut()
                .expect("virtio-net TX queue not configured")
                .poll_completion()
            {
                break completion;
            }
            x86_64::instructions::interrupts::enable_and_hlt();
        };
        if interrupts_were_enabled {
            x86_64::instructions::interrupts::enable();
        }

        assert_eq!(
            completion.0, head as u32,
            "virtio-net TX completed unexpected descriptor"
        );

        let tx_queue = self.inner.tx_queue.as_mut().unwrap();
        tx_queue.free_descriptor(head);
        tx_queue.free_descriptor(tail);

        println!(
            "[net] TX frame completion head={} len={} (descriptor bytes={})",
            completion.0,
            completion.1,
            VIRTIO_NET_HDR_LEN + tx_len
        );
    }

    pub fn recv_frame(&mut self) -> Option<Frame> {
        let (desc_id, written_len) = {
            let rx_queue = self.inner.rx_queue.as_mut()?;
            rx_queue.poll_completion()?
        };

        let desc_idx = desc_id as u16;
        let buffer_index = self
            .inner
            .rx_buffers
            .iter()
            .position(|buffer| buffer.desc_idx == desc_idx)
            .expect("virtio-net RX completion used an unknown descriptor");

        let (buffer_virt, buffer_len) = {
            let buffer = &self.inner.rx_buffers[buffer_index];
            (buffer.virt, buffer.len as usize)
        };

        let rx_len = core::cmp::min(written_len as usize, buffer_len);
        let frame = if rx_len >= VIRTIO_NET_HDR_LEN + ETHERNET_HEADER_LEN {
            let eth_start = buffer_virt + VIRTIO_NET_HDR_LEN as u64;
            let eth_len = rx_len - VIRTIO_NET_HDR_LEN;
            let mut header_bytes = [0u8; ETHERNET_HEADER_LEN];
            unsafe {
                for (i, byte) in header_bytes.iter_mut().enumerate() {
                    *byte = read_volatile((eth_start + i as u64) as *const u8);
                }
            }
            let header = EthernetHeader::parse(&header_bytes)
                .expect("Ethernet header was length-checked above");
            let payload_len = core::cmp::min(eth_len - ETHERNET_HEADER_LEN, ETHERNET_MTU);
            let payload_start = eth_start + ETHERNET_HEADER_LEN as u64;
            let frame = Frame::from_header_and_payload(header, payload_start, payload_len);

            println!(
                "[net] RX ethertype=0x{:04x} src={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} dst={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} payload_len={}",
                frame.ethertype,
                frame.src[0],
                frame.src[1],
                frame.src[2],
                frame.src[3],
                frame.src[4],
                frame.src[5],
                frame.dst[0],
                frame.dst[1],
                frame.dst[2],
                frame.dst[3],
                frame.dst[4],
                frame.dst[5],
                frame.payload_len
            );
            Some(frame)
        } else {
            println!(
                "[net] RX dropped short virtio-net buffer desc={} len={}",
                desc_idx, rx_len
            );
            None
        };

        self.inner.recycle_rx_buffer(desc_idx);
        frame
    }

    pub fn poll_rx(&mut self) -> Option<NetRxEvent> {
        let frame = self.recv_frame()?;
        Some(self.dispatch_frame(&frame))
    }

    fn dispatch_frame(&mut self, frame: &Frame) -> NetRxEvent {
        if let Some(packet) = self.handle_arp_frame(frame) {
            return NetRxEvent::Arp(packet);
        }

        if let Some(packet) = self.handle_ipv4_frame(frame) {
            if packet.header.protocol == net::ipv4::PROTOCOL_TCP {
                if let Some(reply) =
                    net::tcp::receive(packet.header.src_ip, packet.header.dst_ip, packet.payload())
                {
                    let _ =
                        self.send_ipv4_packet(net::ipv4::PROTOCOL_TCP, reply.dst_ip, &reply.bytes);
                }
                return NetRxEvent::Tcp;
            }
            if packet.header.protocol == net::ipv4::PROTOCOL_UDP {
                let udp_bytes = packet.payload();
                let Some(datagram) =
                    UdpDatagram::parse(udp_bytes, packet.header.src_ip, packet.header.dst_ip)
                else {
                    println!("[udp] dropped malformed datagram");
                    return NetRxEvent::Other;
                };

                println!(
                    "[udp] rx src={}.{}.{}.{}:{} dst={}.{}.{}.{}:{} len={} payload_len={} checksum ok",
                    packet.header.src_ip[0],
                    packet.header.src_ip[1],
                    packet.header.src_ip[2],
                    packet.header.src_ip[3],
                    datagram.header.src_port,
                    packet.header.dst_ip[0],
                    packet.header.dst_ip[1],
                    packet.header.dst_ip[2],
                    packet.header.dst_ip[3],
                    datagram.header.dst_port,
                    datagram.header.length,
                    datagram.payload.len()
                );

                let mut payload = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN - UDP_HEADER_LEN];
                payload[..datagram.payload.len()].copy_from_slice(datagram.payload);
                let udp_packet = UdpPacket {
                    ip_header: packet.header,
                    header: datagram.header,
                    payload,
                    payload_len: datagram.payload.len(),
                };
                crate::net::socket::deliver(
                    udp_packet.header.dst_port,
                    crate::net::socket::Endpoint {
                        ip: udp_packet.ip_header.src_ip,
                        port: udp_packet.header.src_port,
                    },
                    udp_packet.payload(),
                );
                return NetRxEvent::Udp(udp_packet);
            }

            return NetRxEvent::Ipv4(packet);
        }

        NetRxEvent::Other
    }

    /// Resolve `ip` to a MAC address, sleeping until the reply arrives.
    ///
    /// Used where no network task exists (boot, before `network_task` is
    /// spawned), so this function drains the RX ring itself after every
    /// wakeup. The RX MSI-X interrupt — or the 100 Hz timer as a
    /// backstop — ends each `hlt`. Syscall-driven TX paths use
    /// [`resolve_arp_blocking`] instead, which never holds the device
    /// lock while sleeping.
    pub fn resolve_arp(&mut self, ip: [u8; 4]) -> Option<[u8; 6]> {
        if let Some(mac) = arp::lookup(ip) {
            println!(
                "[arp] cache hit: {}.{}.{}.{} is at {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                ip[0], ip[1], ip[2], ip[3], mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            );
            return Some(mac);
        }

        println!(
            "[arp] cache miss: asking for {}.{}.{}.{}",
            ip[0], ip[1], ip[2], ip[3]
        );
        self.send_arp_request(ip);

        for _ in 0..500u32 {
            x86_64::instructions::interrupts::disable();
            // Dispatch whatever arrived, then re-check the cache. The
            // reply we need may already sit in the RX used ring.
            let _ = self.poll_rx();
            if let Some(mac) = arp::lookup(ip) {
                x86_64::instructions::interrupts::enable();
                println!(
                    "[arp] {}.{}.{}.{} is at {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    ip[0], ip[1], ip[2], ip[3], mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                );
                return Some(mac);
            }
            x86_64::instructions::interrupts::enable_and_hlt();
        }

        println!(
            "[arp] resolve timed out for {}.{}.{}.{}",
            ip[0], ip[1], ip[2], ip[3]
        );
        None
    }

    pub fn send_ipv4_packet(
        &mut self,
        protocol: u8,
        dst_ip: [u8; 4],
        payload: &[u8],
    ) -> Option<()> {
        assert!(
            payload.len() <= ETHERNET_MTU - IPV4_HEADER_LEN,
            "IPv4 payload {} exceeds Ethernet MTU room {}",
            payload.len(),
            ETHERNET_MTU - IPV4_HEADER_LEN
        );

        let broadcast = dst_ip == [255; 4];
        let next_hop_ip = next_hop_for(dst_ip);
        // ARP is resolved before this function is called, either at boot
        // (gateway entry) or by `resolve_arp_blocking` on the syscall
        // path. Sleeping here would deadlock: the network task needs this
        // same device lock to deliver the ARP reply we're waiting for.
        // On a miss, send an ARP request to prime the cache and drop the
        // packet — the caller's retry or the next packet gets through.
        let next_hop_mac = if broadcast {
            [0xff; 6]
        } else if let Some(mac) = arp::lookup(next_hop_ip) {
            mac
        } else {
            println!(
                "[arp] no cache entry for next hop {}.{}.{}.{} — priming and dropping packet",
                next_hop_ip[0], next_hop_ip[1], next_hop_ip[2], next_hop_ip[3]
            );
            self.send_arp_request(next_hop_ip);
            return None;
        };
        let source_ip = net::source_ip();
        let ip_header = Ipv4Header::build(protocol, source_ip, dst_ip, payload.len());

        let ethernet_header = EthernetHeader {
            dst: next_hop_mac,
            src: self.mac(),
            ethertype: ETHERTYPE_IPV4,
        };

        let frame_len = ETHERNET_HEADER_LEN + IPV4_HEADER_LEN + payload.len();
        let mut frame = [0u8; ETHERNET_HEADER_LEN + ETHERNET_MTU];
        frame[0..ETHERNET_HEADER_LEN].copy_from_slice(&ethernet_header.to_bytes());
        frame[ETHERNET_HEADER_LEN..ETHERNET_HEADER_LEN + IPV4_HEADER_LEN]
            .copy_from_slice(&ip_header);
        frame[ETHERNET_HEADER_LEN + IPV4_HEADER_LEN..frame_len].copy_from_slice(payload);

        self.send_frame(&frame[..frame_len]);
        println!(
            "[ip] tx proto={} src={}.{}.{}.{} dst={}.{}.{}.{} len={} next-hop={}.{}.{}.{} ({:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
            protocol,
            source_ip[0], source_ip[1], source_ip[2], source_ip[3],
            dst_ip[0],
            dst_ip[1],
            dst_ip[2],
            dst_ip[3],
            IPV4_HEADER_LEN + payload.len(),
            next_hop_ip[0],
            next_hop_ip[1],
            next_hop_ip[2],
            next_hop_ip[3],
            next_hop_mac[0],
            next_hop_mac[1],
            next_hop_mac[2],
            next_hop_mac[3],
            next_hop_mac[4],
            next_hop_mac[5]
        );

        Some(())
    }

    /// Build and send one UDP datagram through the IPv4 and Ethernet paths.
    pub fn send_udp(
        &mut self,
        dst_ip: [u8; 4],
        dst_port: u16,
        src_port: u16,
        payload: &[u8],
    ) -> Option<()> {
        let mut datagram = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN];
        let datagram_len = udp::write_datagram(
            &mut datagram,
            net::source_ip(),
            dst_ip,
            src_port,
            dst_port,
            payload,
        )?;
        self.send_ipv4_packet(net::ipv4::PROTOCOL_UDP, dst_ip, &datagram[..datagram_len])
    }

    /// Send one ARP request for `target_ip`. Public so the blocking
    /// resolver can prime the cache without owning the device.
    pub fn send_arp_request(&mut self, target_ip: [u8; 4]) {
        let packet = ArpPacket::request(self.mac(), net::source_ip(), target_ip);
        self.send_arp_packet([0xff; 6], packet);
    }

    fn send_arp_reply(&mut self, request: &ArpPacket) {
        let reply = ArpPacket::reply_to(request, self.mac(), net::source_ip());
        self.send_arp_packet(request.sha, reply);
        println!(
            "[arp] replied: {}.{}.{}.{} is at {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            net::source_ip()[0],
            net::source_ip()[1],
            net::source_ip()[2],
            net::source_ip()[3],
            self.mac()[0],
            self.mac()[1],
            self.mac()[2],
            self.mac()[3],
            self.mac()[4],
            self.mac()[5]
        );
    }

    fn send_arp_packet(&mut self, dst_mac: [u8; 6], packet: ArpPacket) {
        let header = EthernetHeader {
            dst: dst_mac,
            src: self.mac(),
            ethertype: ETHERTYPE_ARP,
        };
        let mut frame = [0u8; ETHERNET_HEADER_LEN + ARP_PACKET_LEN];
        frame[0..ETHERNET_HEADER_LEN].copy_from_slice(&header.to_bytes());
        frame[ETHERNET_HEADER_LEN..].copy_from_slice(&packet.to_bytes());

        self.send_frame(&frame);
        println!(
            "[net] TX broadcast frame sent, {} bytes",
            VIRTIO_NET_HDR_LEN + frame.len()
        );
    }

    fn handle_arp_frame(&mut self, frame: &Frame) -> Option<ArpPacket> {
        if frame.ethertype != ETHERTYPE_ARP {
            return None;
        }

        let packet = ArpPacket::parse(&frame.payload[..frame.payload_len])?;
        if !packet.is_ipv4_over_ethernet() {
            println!("[arp] dropped unsupported ARP packet");
            return None;
        }

        arp::merge(&packet);
        println!(
            "[arp] learned {}.{}.{}.{} -> {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            packet.spa[0],
            packet.spa[1],
            packet.spa[2],
            packet.spa[3],
            packet.sha[0],
            packet.sha[1],
            packet.sha[2],
            packet.sha[3],
            packet.sha[4],
            packet.sha[5]
        );

        if packet.oper == OPER_REQUEST
            && net::config()
                .map(|c| packet.tpa == c.address)
                .unwrap_or(false)
        {
            self.send_arp_reply(&packet);
        }

        Some(packet)
    }

    pub fn handle_ipv4_frame(&mut self, frame: &Frame) -> Option<Ipv4Packet> {
        if frame.ethertype != ETHERTYPE_IPV4 {
            return None;
        }

        let header = Ipv4Header::parse(&frame.payload[..frame.payload_len])?;
        if !net::accepts_destination(header.dst_ip) {
            println!(
                "[ip] dropped frame for {}.{}.{}.{}",
                header.dst_ip[0], header.dst_ip[1], header.dst_ip[2], header.dst_ip[3]
            );
            return None;
        }

        println!(
            "[ip] rx proto={} src={}.{}.{}.{} dst={}.{}.{}.{} len={}",
            header.protocol,
            header.src_ip[0],
            header.src_ip[1],
            header.src_ip[2],
            header.src_ip[3],
            header.dst_ip[0],
            header.dst_ip[1],
            header.dst_ip[2],
            header.dst_ip[3],
            header.total_len
        );

        let payload_len = header.payload_len();
        let ip_payload = &frame.payload[IPV4_HEADER_LEN..IPV4_HEADER_LEN + payload_len];

        if header.protocol == net::ipv4::PROTOCOL_ICMP {
            if let Some(request) = net::icmp::EchoMessage::parse(ip_payload) {
                if request.message_type == net::icmp::ECHO_REQUEST
                    && request.code == net::icmp::ECHO_CODE
                {
                    let mut reply = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN];
                    let reply_len = net::icmp::write_echo_reply(
                        &mut reply,
                        request.identifier,
                        request.sequence,
                        request.payload,
                    )
                    .expect("maximum IPv4 payload fits in an Ethernet frame");

                    if self
                        .send_ipv4_packet(
                            net::ipv4::PROTOCOL_ICMP,
                            header.src_ip,
                            &reply[..reply_len],
                        )
                        .is_some()
                    {
                        println!(
                            "[icmp] echoed request id=0x{:04x} seq={} to {}.{}.{}.{}",
                            request.identifier,
                            request.sequence,
                            header.src_ip[0],
                            header.src_ip[1],
                            header.src_ip[2],
                            header.src_ip[3]
                        );
                    }
                }
            }
        }

        let mut payload = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN];
        payload[..payload_len].copy_from_slice(ip_payload);

        Some(Ipv4Packet {
            header,
            payload,
            payload_len,
        })
    }

    fn handle_udp_packet(&mut self, packet: &UdpPacket) {
        if packet.header.dst_port != udp::ECHO_PORT {
            println!(
                "[udp] no receiver for destination port {}",
                packet.header.dst_port
            );
            return;
        }

        if self
            .send_udp(
                packet.ip_header.src_ip,
                packet.header.src_port,
                udp::ECHO_PORT,
                packet.payload(),
            )
            .is_some()
        {
            println!(
                "[udp] echoed port {} payload_len={} to {}.{}.{}.{}:{}",
                udp::ECHO_PORT,
                packet.payload().len(),
                packet.ip_header.src_ip[0],
                packet.ip_header.src_ip[1],
                packet.ip_header.src_ip[2],
                packet.ip_header.src_ip[3],
                packet.header.src_port
            );
            println!(
                "[udp] *** guest port {} echoed one datagram ***",
                udp::ECHO_PORT
            );
        } else {
            println!("[udp] echo transmit failed");
        }
    }
}

fn next_hop_for(dst_ip: [u8; 4]) -> [u8; 4] {
    let config = net::config();
    if let Some(value) = config {
        if dst_ip[0] & value.subnet_mask[0] == value.address[0] & value.subnet_mask[0]
            && dst_ip[1] & value.subnet_mask[1] == value.address[1] & value.subnet_mask[1]
            && dst_ip[2] & value.subnet_mask[2] == value.address[2] & value.subnet_mask[2]
            && dst_ip[3] & value.subnet_mask[3] == value.address[3] & value.subnet_mask[3]
        {
            dst_ip
        } else {
            value.gateway
        }
    } else {
        dst_ip
    }
}

/// Resolve the next hop for `dst_ip`, sleeping until the ARP reply
/// arrives. For syscall TX paths (`sendto`): the device lock is held
/// only for the request transmit, never while sleeping, so the network
/// task can deliver the reply into the ARP cache.
pub fn resolve_next_hop(dst_ip: [u8; 4]) -> Option<[u8; 6]> {
    resolve_arp_blocking(next_hop_for(dst_ip))
}

/// Blocking ARP resolve that never holds the NET_DEVICE lock while
/// sleeping. Sends one request, then `hlt`-sleeps until the network task
/// (woken by the RX interrupt) delivers the reply into the cache. The
/// 100 Hz timer is the backstop that re-checks the cache at least every
/// 10 ms even if an interrupt is somehow missed.
fn resolve_arp_blocking(ip: [u8; 4]) -> Option<[u8; 6]> {
    if let Some(mac) = arp::lookup(ip) {
        return Some(mac);
    }

    println!(
        "[arp] syscall path miss: asking for {}.{}.{}.{}",
        ip[0], ip[1], ip[2], ip[3]
    );
    let _ = with_net_device(|device| device.send_arp_request(ip));

    for _ in 0..500u32 {
        x86_64::instructions::interrupts::disable();
        if let Some(mac) = arp::lookup(ip) {
            x86_64::instructions::interrupts::enable();
            println!(
                "[arp] {}.{}.{}.{} is at {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                ip[0], ip[1], ip[2], ip[3], mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            );
            return Some(mac);
        }
        x86_64::instructions::interrupts::enable_and_hlt();
    }

    println!(
        "[arp] syscall path resolve timed out for {}.{}.{}.{}",
        ip[0], ip[1], ip[2], ip[3]
    );
    None
}

/// DMA buffers for one virtio-net TX frame.
struct NetTxBuffers {
    header_phys: u64,
    frame_phys: u64,
}

impl NetTxBuffers {
    fn new(eth_frame: &[u8]) -> Self {
        assert!(
            eth_frame.len() <= 4096,
            "single-frame TX buffer cannot hold {} bytes",
            eth_frame.len()
        );

        let header_frame =
            crate::frame_allocator::allocate().expect("out of frames for virtio-net TX header");
        let frame_frame =
            crate::frame_allocator::allocate().expect("out of frames for virtio-net TX frame");
        let offset = crate::memory::phys_offset();
        let header_phys = header_frame * 4096;
        let frame_phys = frame_frame * 4096;
        let header_virt = header_phys + offset;
        let frame_virt = frame_phys + offset;

        unsafe {
            core::ptr::write_bytes(header_virt as *mut u8, 0, 4096);
            core::ptr::write_bytes(frame_virt as *mut u8, 0, 4096);
            core::ptr::copy_nonoverlapping(
                eth_frame.as_ptr(),
                frame_virt as *mut u8,
                eth_frame.len(),
            );
        }

        Self {
            header_phys,
            frame_phys,
        }
    }
}

/// Driver state for the virtio-net device after PCI probing.
pub struct VirtioNet {
    bus: u8,
    device: u8,
    func: u8,
    common_virt: u64,
    notify_virt: u64,
    notify_off_multiplier: u32,
    device_config_virt: u64,
    mac: [u8; 6],
    rx_queue: Option<VirtQueue>,
    tx_queue: Option<VirtQueue>,
    rx_buffers: alloc::vec::Vec<RxBuffer>,
    msix: MsixCap,
}

impl VirtioNet {
    /// Find the modern virtio-net PCI device and map the common,
    /// notification, and device-config capability regions.
    pub fn probe() -> Option<Self> {
        let dev = pci::find_virtio_device(pci::VIRTIO_NET_NON_TRANSITIONAL)?;
        let caps = parse_virtio_capabilities(dev.bus, dev.device, dev.func);
        let msix = parse_msix_capability(dev.bus, dev.device, dev.func)?;

        let common_cap = caps.iter().find(|c| c.cfg_type == VIRTIO_CAP_COMMON)?;
        let common_bar_base = pci::bar_base(dev.bus, dev.device, dev.func, common_cap.bar);
        let common_phys = common_bar_base + common_cap.offset as u64;
        let common_virt = crate::memory::map_mmio(common_phys, common_cap.length as u64);
        println!(
            "[virtio-net] common config: BAR{}+0x{:X} = phys 0x{:X} = virt 0x{:X}",
            common_cap.bar, common_cap.offset, common_phys, common_virt
        );

        let notify_cap = caps.iter().find(|c| c.cfg_type == VIRTIO_CAP_NOTIFY)?;
        let notify_bar_base = pci::bar_base(dev.bus, dev.device, dev.func, notify_cap.bar);
        let notify_phys = notify_bar_base + notify_cap.offset as u64;
        let notify_virt = crate::memory::map_mmio(notify_phys, notify_cap.length as u64);
        println!(
            "[virtio-net] notifications: BAR{}+0x{:X} = phys 0x{:X} = virt 0x{:X} (multiplier {})",
            notify_cap.bar,
            notify_cap.offset,
            notify_phys,
            notify_virt,
            notify_cap.notify_off_multiplier,
        );

        let device_config_cap = caps
            .iter()
            .find(|c| c.cfg_type == VIRTIO_CAP_DEVICE_CONFIG)?;
        let device_config_bar_base =
            pci::bar_base(dev.bus, dev.device, dev.func, device_config_cap.bar);
        let device_config_phys = device_config_bar_base + device_config_cap.offset as u64;
        let device_config_virt =
            crate::memory::map_mmio(device_config_phys, device_config_cap.length as u64);
        println!(
            "[virtio-net] device config: BAR{}+0x{:X} = phys 0x{:X} = virt 0x{:X}",
            device_config_cap.bar, device_config_cap.offset, device_config_phys, device_config_virt
        );

        Some(Self {
            bus: dev.bus,
            device: dev.device,
            func: dev.func,
            common_virt,
            notify_virt,
            notify_off_multiplier: notify_cap.notify_off_multiplier,
            device_config_virt,
            mac: [0; 6],
            rx_queue: None,
            tx_queue: None,
            rx_buffers: alloc::vec::Vec::new(),
            msix,
        })
    }

    fn read_common_u8(&self, offset: u8) -> u8 {
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u8) }
    }

    fn write_common_u8(&self, offset: u8, value: u8) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u8, value);
        }
    }

    fn read_common_u16(&self, offset: u8) -> u16 {
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u16) }
    }

    fn write_common_u16(&self, offset: u8, value: u16) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u16, value);
        }
    }

    fn read_common_u32(&self, offset: u8) -> u32 {
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u32) }
    }

    fn write_common_u32(&self, offset: u8, value: u32) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u32, value);
        }
    }

    fn write_common_u64(&self, offset: u8, value: u64) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u64, value);
        }
    }

    pub fn read_status(&self) -> u8 {
        self.read_common_u8(COMMON_DEVICE_STATUS)
    }

    pub fn write_status(&self, value: u8) {
        self.write_common_u8(COMMON_DEVICE_STATUS, value);
    }

    pub fn reset_and_acknowledge(&self) {
        let before = self.read_status();
        println!(
            "[virtio-net] {:02x}:{:02x}.{} device_status before reset: 0x{:02X}",
            self.bus, self.device, self.func, before
        );

        self.write_status(0);
        let after_reset = self.read_status();
        println!(
            "[virtio-net] device_status after reset:  0x{:02X}",
            after_reset
        );
        assert_eq!(after_reset, 0, "virtio-net did not reset cleanly");

        self.write_status(STATUS_ACKNOWLEDGE);
        let after_ack = self.read_status();
        println!(
            "[virtio-net] device_status after ACK:    0x{:02X}",
            after_ack
        );

        self.write_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        let after_driver = self.read_status();
        println!(
            "[virtio-net] device_status after DRIVER: 0x{:02X}",
            after_driver
        );
    }

    /// Negotiate the modern transport bit plus NET_F_MAC.
    pub fn negotiate_features(&self) {
        self.write_common_u32(COMMON_DEVICE_FEATURE_SELECT, 0);
        let device_features_low = self.read_common_u32(COMMON_DEVICE_FEATURE);
        self.write_common_u32(COMMON_DEVICE_FEATURE_SELECT, 1);
        let device_features_high = self.read_common_u32(COMMON_DEVICE_FEATURE);

        println!(
            "[virtio-net] device features low:  0x{:08X}",
            device_features_low
        );
        println!(
            "[virtio-net] device features high: 0x{:08X}",
            device_features_high
        );

        assert!(
            device_features_low & VIRTIO_NET_F_MAC != 0,
            "virtio-net did not advertise VIRTIO_NET_F_MAC"
        );
        assert!(
            device_features_high & 0x1 != 0,
            "virtio-net did not advertise VIRTIO_F_VERSION_1"
        );

        let supported_low = VIRTIO_NET_F_MAC;
        let supported_high = 0x1;

        println!(
            "[virtio-net] driver features low:  0x{:08X} (VIRTIO_NET_F_MAC)",
            supported_low
        );
        println!(
            "[virtio-net] driver features high: 0x{:08X} (VIRTIO_F_VERSION_1)",
            supported_high
        );

        self.write_common_u32(COMMON_DRIVER_FEATURE_SELECT, 0);
        self.write_common_u32(COMMON_DRIVER_FEATURE, supported_low);
        self.write_common_u32(COMMON_DRIVER_FEATURE_SELECT, 1);
        self.write_common_u32(COMMON_DRIVER_FEATURE, supported_high);

        self.write_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);
        let status = self.read_status();
        println!(
            "[virtio-net] device_status after FEATURES_OK: 0x{:02X}",
            status
        );
        assert!(
            status & STATUS_FEATURES_OK != 0,
            "virtio-net rejected our feature subset"
        );
    }

    /// Read and store the MAC address from device config offset 0.
    pub fn read_mac(&mut self) -> [u8; 6] {
        let mut mac = [0u8; 6];
        for (i, byte) in mac.iter_mut().enumerate() {
            *byte = unsafe { read_volatile((self.device_config_virt + i as u64) as *const u8) };
        }
        self.mac = mac;
        println!(
            "[virtio-net] MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
        mac
    }

    pub fn setup_queues(&mut self, queue_size: u16) {
        let rx_queue = VirtQueue::new(queue_size);
        self.configure_queue_at(0, &rx_queue, "RX");
        self.rx_queue = Some(rx_queue);

        let tx_queue = VirtQueue::new(queue_size);
        self.configure_queue_at(1, &tx_queue, "TX");
        self.tx_queue = Some(tx_queue);

        self.configure_queue_interrupts();
    }

    /// Point each queue at its own MSI-X table entry: RX queue 0 fires
    /// vector 0x30, TX queue 1 fires vector 0x31. MSI-X tables belong to
    /// individual PCI functions, so virtio-net starts at its own entry 0;
    /// virtio-blk's entry 0 lives in a different device's table.
    fn configure_queue_interrupts(&self) {
        assert!(
            self.msix.entries >= 2,
            "virtio-net MSI-X table has {} entries, need at least 2 (RX + TX)",
            self.msix.entries
        );
        for (entry_index, vector) in [
            (0u64, crate::interrupts::VIRTIO_NET_RX_VECTOR),
            (1u64, crate::interrupts::VIRTIO_NET_TX_VECTOR),
        ] {
            let entry = self.msix.table_virt + entry_index * 16;
            unsafe {
                write_volatile(entry as *mut u32, crate::apic::MSI_ADDRESS);
                write_volatile((entry + 4) as *mut u32, 0);
                write_volatile((entry + 8) as *mut u32, vector as u32);
                write_volatile((entry + 12) as *mut u32, 0);
            }
        }

        self.write_common_u16(COMMON_QUEUE_SELECT, 0);
        self.write_common_u16(COMMON_QUEUE_MSIX_VECTOR, 0);
        let rx_assigned = self.read_common_u16(COMMON_QUEUE_MSIX_VECTOR);
        assert_eq!(rx_assigned, 0, "virtio-net RX queue rejected MSI-X entry 0");
        println!(
            "[virtio-net] MSI-X queue 0 (RX) -> table 0 -> vector 0x{:02x}",
            crate::interrupts::VIRTIO_NET_RX_VECTOR
        );

        self.write_common_u16(COMMON_QUEUE_SELECT, 1);
        self.write_common_u16(COMMON_QUEUE_MSIX_VECTOR, 1);
        let tx_assigned = self.read_common_u16(COMMON_QUEUE_MSIX_VECTOR);
        assert_eq!(tx_assigned, 1, "virtio-net TX queue rejected MSI-X entry 1");
        println!(
            "[virtio-net] MSI-X queue 1 (TX) -> table 1 -> vector 0x{:02x}",
            crate::interrupts::VIRTIO_NET_TX_VECTOR
        );
    }

    fn configure_queue_at(&self, queue_index: u16, queue: &VirtQueue, label: &str) {
        println!("[virtio-net] configuring queue {} ({})", queue_index, label);

        self.write_common_u16(COMMON_QUEUE_SELECT, queue_index);
        let device_max = self.read_common_u16(COMMON_QUEUE_SIZE);
        println!(
            "[virtio-net]   queue {} max size: {} (we want {})",
            queue_index,
            device_max,
            queue.queue_size()
        );
        assert!(
            queue.queue_size() <= device_max,
            "virtio-net queue {} max size {} < requested {}",
            queue_index,
            device_max,
            queue.queue_size()
        );

        self.write_common_u16(COMMON_QUEUE_SIZE, queue.queue_size());
        self.write_common_u64(COMMON_QUEUE_DESC, queue.desc_phys());
        self.write_common_u64(COMMON_QUEUE_AVAIL, queue.avail_phys());
        self.write_common_u64(COMMON_QUEUE_USED, queue.used_phys());
        self.write_common_u16(COMMON_QUEUE_ENABLE, 1);

        let enable_readback = self.read_common_u16(COMMON_QUEUE_ENABLE);
        println!(
            "[virtio-net]   queue {} enable readback: {}",
            queue_index, enable_readback
        );
        assert_eq!(
            enable_readback, 1,
            "virtio-net queue {} was rejected",
            queue_index
        );
        println!("[virtio-net] queue {} ({}) active", queue_index, label);
    }

    /// Allocate and publish the initial receive-buffer supply.
    pub fn post_initial_rx_buffers(&mut self) {
        let rx_queue = self
            .rx_queue
            .as_mut()
            .expect("virtio-net RX queue not configured");
        assert!(
            rx_queue.num_free() >= VIRTIO_NET_RX_BUFFER_COUNT,
            "not enough RX descriptors for initial supply"
        );

        for _ in 0..VIRTIO_NET_RX_BUFFER_COUNT {
            let frame =
                crate::frame_allocator::allocate().expect("out of frames for virtio-net RX buffer");
            let phys = frame * 4096;
            let virt = phys + crate::memory::phys_offset();

            unsafe {
                core::ptr::write_bytes(virt as *mut u8, 0, 4096);
            }

            let desc_idx = rx_queue.alloc_descriptor().expect("no free RX descriptors");
            rx_queue.write_descriptor(
                desc_idx,
                phys,
                VIRTIO_NET_RX_BUFFER_LEN,
                VIRTQ_DESC_F_WRITE,
                0,
            );
            rx_queue.submit_chain(desc_idx);

            self.rx_buffers.push(RxBuffer {
                desc_idx,
                virt,
                phys,
                len: VIRTIO_NET_RX_BUFFER_LEN,
                frame,
            });
        }

        let (avail_idx, used_idx, last_used_idx) = rx_queue.debug_state();
        if let Some(first) = self.rx_buffers.first() {
            println!(
                "[virtio-net] first RX buffer: desc={} frame={} phys=0x{:X} virt=0x{:X} len={}",
                first.desc_idx, first.frame, first.phys, first.virt, first.len
            );
        }
        println!(
            "[virtio-net] posted {} RX buffers (len={} bytes each, avail_idx={}, used_idx={}, last_used_idx={})",
            self.rx_buffers.len(),
            VIRTIO_NET_RX_BUFFER_LEN,
            avail_idx,
            used_idx,
            last_used_idx
        );
    }

    pub fn set_driver_ok(&self) {
        self.write_status(
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );
        let status = self.read_status();
        println!(
            "[virtio-net] device_status after DRIVER_OK: 0x{:02X}",
            status
        );
        assert!(
            status & STATUS_FAILED == 0,
            "virtio-net entered FAILED state"
        );
        assert!(
            status & STATUS_DEVICE_NEEDS_RESET == 0,
            "virtio-net requested reset"
        );
        assert!(
            status & STATUS_DRIVER_OK != 0,
            "virtio-net DRIVER_OK did not stick"
        );
        println!(
            "[virtio-net] device is live with RX stocked and TX idle (notify=0x{:X}, multiplier={})",
            self.notify_virt,
            self.notify_off_multiplier
        );
    }

    fn recycle_rx_buffer(&mut self, desc_idx: u16) {
        let buffer = self
            .rx_buffers
            .iter()
            .find(|buffer| buffer.desc_idx == desc_idx)
            .expect("cannot recycle unknown RX descriptor");

        unsafe {
            core::ptr::write_bytes(buffer.virt as *mut u8, 0, buffer.len as usize);
        }

        {
            let rx_queue = self
                .rx_queue
                .as_mut()
                .expect("virtio-net RX queue not configured");
            rx_queue.submit_chain(desc_idx);
            let (avail_idx, used_idx, last_used_idx) = rx_queue.debug_state();
            println!(
                "[net] RX buffer desc={} recycled (avail_idx={}, used_idx={}, last_used_idx={})",
                desc_idx, avail_idx, used_idx, last_used_idx
            );
        }

        self.kick_queue(0);
    }

    fn kick_queue(&self, queue_index: u16) {
        self.write_common_u16(COMMON_QUEUE_SELECT, queue_index);
        let queue_notify_off = self.read_common_u16(COMMON_QUEUE_NOTIFY_OFF) as u64;
        let off = queue_notify_off * (self.notify_off_multiplier as u64);
        let notify_addr = self.notify_virt + off;

        unsafe {
            write_volatile(notify_addr as *mut u16, queue_index);
        }
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
pub const VIRTIO_BLK_T_IN: u32 = 0; // read
pub const VIRTIO_BLK_T_OUT: u32 = 1; // write
pub const VIRTIO_BLK_T_FLUSH: u32 = 4; // flush cache

/// virtio-blk status byte values — written by the device on completion.
pub const VIRTIO_BLK_S_OK: u8 = 0;
pub const VIRTIO_BLK_S_IOERR: u8 = 1;
pub const VIRTIO_BLK_S_UNSUPP: u8 = 2;

/// virtio-blk request header. Sent as the first buffer in every request.
/// `repr(C)` so fields land at the spec-mandated offsets.
///
/// After this struct, the chain continues with the data buffer (for the
/// bytes being read or written) and a one-byte status field. The device
/// reads the header, performs the operation, fills the data buffer (for
/// reads), and writes the status byte.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
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

/// DMA-able buffer set for one virtio-blk request: header, data, status.
///
/// Each buffer is allocated as its own 4 KiB frame from the frame
/// allocator — wasteful (16 B header in a 4096 B frame) but the
/// simplest path to a physical address. Linux's `kmalloc` would
/// sub-allocate these into a shared page; that's a future optimization.
///
/// Frames are tracked (not exposed publicly) so a future `Drop` impl
/// can return them to the frame allocator. For now the kernel lives
/// forever, so we leak — fine for early-boot code.
pub struct BlkBuffers {
    /// Virtual addresses — kernel-side access via the direct map.
    pub header_virt: u64,
    pub data_virt: u64,
    pub status_virt: u64,

    /// Physical addresses — written into descriptors for the device.
    pub header_phys: u64,
    pub data_phys: u64,
    pub status_phys: u64,

    // Frame numbers (for future cleanup).
    header_frame: u64,
    data_frame: u64,
    status_frame: u64,
}

impl BlkBuffers {
    /// Allocate one fresh frame for each of header / data / status, zero
    /// them, and return the bundle. Caller writes the request header
    /// into `header_virt` before submitting.
    pub fn new() -> Self {
        let header_frame =
            crate::frame_allocator::allocate().expect("out of frames for blk request header");
        let data_frame =
            crate::frame_allocator::allocate().expect("out of frames for blk data buffer");
        let status_frame =
            crate::frame_allocator::allocate().expect("out of frames for blk status byte");

        let offset = crate::memory::phys_offset();
        let header_phys = header_frame * 4096;
        let data_phys = data_frame * 4096;
        let status_phys = status_frame * 4096;

        let header_virt = header_phys + offset;
        let data_virt = data_phys + offset;
        let status_virt = status_phys + offset;

        // Zero everything. The header is overwritten below; the data
        // buffer needs to start zeroed so verification can distinguish
        // "device wrote zeros" from "stale allocator garbage"; the
        // status byte starts at 0 so we can detect device failure to
        // respond.
        // SAFETY: all three frames freshly allocated, reachable via
        // the direct map, no aliases.
        unsafe {
            core::ptr::write_bytes(header_virt as *mut u8, 0, 4096);
            core::ptr::write_bytes(data_virt as *mut u8, 0, 4096);
            core::ptr::write_bytes(status_virt as *mut u8, 0, 4096);
        }

        Self {
            header_virt,
            data_virt,
            status_virt,
            header_phys,
            data_phys,
            status_phys,
            header_frame,
            data_frame,
            status_frame,
        }
    }

    /// Write a `VirtioBlkReq` into the header buffer. Volatile so the
    /// compiler doesn't elide the store before the device reads it.
    pub fn write_header(&self, req: &VirtioBlkReq) {
        // SAFETY: header_virt points at a 4096-byte buffer that we own.
        // VirtioBlkReq is 16 bytes, fits comfortably. repr(C) ensures
        // the on-the-wire layout matches.
        unsafe {
            let ptr = self.header_virt as *mut VirtioBlkReq;
            core::ptr::write_volatile(ptr, *req);
        }
    }

    /// Read the 1-byte status from the status buffer. Should be called
    /// only after `poll_completion` returns Some for this request's head.
    pub fn read_status(&self) -> u8 {
        // SAFETY: status_virt points at a 4096-byte buffer; reading the
        // first byte is in bounds.
        unsafe { core::ptr::read_volatile(self.status_virt as *const u8) }
    }

    /// Snapshot of the first N bytes of the data buffer, for verification.
    /// Returns a fresh Vec so the caller can compare/inspect at leisure.
    pub fn read_data_prefix(&self, n: usize) -> alloc::vec::Vec<u8> {
        let mut v = alloc::vec::Vec::with_capacity(n);
        // SAFETY: data_virt points at a 4096-byte buffer; n is bounded
        // by the caller (we'll assert).
        assert!(n <= 4096, "read_data_prefix asked for too many bytes");
        unsafe {
            for i in 0..n {
                v.push(core::ptr::read_volatile(
                    (self.data_virt + i as u64) as *const u8,
                ));
            }
        }
        v
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
        let avail_bytes = 6 + q * 2; // flags + idx + ring + used_event
        let used_bytes = 6 + q * 8; // flags + idx + ring entries + avail_event

        // Assert our single-frame-per-region assumption. Larger queues
        // would need multi-frame allocations; we don't need that yet.
        assert!(
            desc_bytes <= 4096,
            "descriptor table doesn't fit in one frame"
        );
        assert!(
            avail_bytes <= 4096,
            "available ring doesn't fit in one frame"
        );
        assert!(used_bytes <= 4096, "used ring doesn't fit in one frame");

        let desc_frame =
            crate::frame_allocator::allocate().expect("out of frames for descriptor table");
        let avail_frame =
            crate::frame_allocator::allocate().expect("out of frames for available ring");
        let used_frame = crate::frame_allocator::allocate().expect("out of frames for used ring");

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

    /// Diagnostic: snapshot of (avail_idx, used_idx, last_used_idx) for
    /// debugging queue state. Useful when a request seems to vanish.
    pub fn debug_state(&self) -> (u16, u16, u16) {
        let avail_idx_atomic: *const AtomicU16 = (self.avail_virt + 2) as *const AtomicU16;
        let used_idx_atomic: *const AtomicU16 = (self.used_virt + 2) as *const AtomicU16;
        let avail = unsafe { (*avail_idx_atomic).load(Ordering::Acquire) };
        let used = unsafe { (*used_idx_atomic).load(Ordering::Acquire) };
        (avail, used, self.last_used_idx)
    }

    /// Write the four fields of descriptor `idx`. Used by future steps
    /// to build a descriptor chain before submitting it.
    ///
    /// SAFETY-conscious: writes via volatile so the compiler can't
    /// reorder or coalesce with device reads.
    pub fn write_descriptor(&self, idx: u16, addr: u64, len: u32, flags: u16, next: u16) {
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

    /// Step 8.3: build a 3-descriptor chain for a virtio-blk read.
    ///
    /// Layout (matches the lesson's chain-flow diagram):
    ///   desc[head] addr=header_phys  len=16   flags=F_NEXT          next=mid
    ///   desc[mid]  addr=data_phys    len=512  flags=F_NEXT|F_WRITE  next=tail
    ///   desc[tail] addr=status_phys  len=1    flags=F_WRITE         next=0
    ///
    /// The header has no F_WRITE (device reads it). Data and status
    /// have F_WRITE (device writes them on completion).
    ///
    /// Returns (head, mid, tail) — all three indices so the caller can
    /// free them after completion. `submit_chain` only needs the head;
    /// use `let (head, _, _)` if you don't care about mid/tail.
    pub fn build_read_chain(
        &mut self,
        header_phys: u64,
        data_phys: u64,
        status_phys: u64,
    ) -> (u16, u16, u16) {
        let head = self.alloc_descriptor().expect("no free descriptors");
        let mid = self.alloc_descriptor().expect("no free descriptors");
        let tail = self.alloc_descriptor().expect("no free descriptors");

        self.write_descriptor(head, header_phys, 16, VIRTQ_DESC_F_NEXT, mid);
        self.write_descriptor(
            mid,
            data_phys,
            512,
            VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE,
            tail,
        );
        self.write_descriptor(tail, status_phys, 1, VIRTQ_DESC_F_WRITE, 0);

        // println!(
        //     "[virtio]   desc[head={}] addr=0x{:X} len=16  flags=0x{:02X} (F_NEXT)         next={}",
        //     head, header_phys, VIRTQ_DESC_F_NEXT, mid
        // );
        // println!(
        //     "[virtio]   desc[mid ={}] addr=0x{:X} len=512 flags=0x{:02X} (F_NEXT|F_WRITE) next={}",
        //     mid, data_phys, VIRTQ_DESC_F_NEXT | VIRTQ_DESC_F_WRITE, tail
        // );
        // println!(
        //     "[virtio]   desc[tail={}] addr=0x{:X} len=1   flags=0x{:02X} (F_WRITE)        next=0",
        //     tail, status_phys, VIRTQ_DESC_F_WRITE
        // );

        (head, mid, tail)
    }

    /// Build a 2-descriptor chain for virtio-net transmit.
    ///
    /// Layout:
    ///   desc[head] addr=virtio_net_hdr len=12       flags=F_NEXT next=tail
    ///   desc[tail] addr=Ethernet frame len=eth_len  flags=0      next=0
    pub fn build_tx_chain(
        &mut self,
        header_phys: u64,
        header_len: u32,
        frame_phys: u64,
        frame_len: u32,
    ) -> (u16, u16) {
        let head = self.alloc_descriptor().expect("no free TX descriptors");
        let tail = self.alloc_descriptor().expect("no free TX descriptors");

        self.write_descriptor(head, header_phys, header_len, VIRTQ_DESC_F_NEXT, tail);
        self.write_descriptor(tail, frame_phys, frame_len, 0, 0);

        (head, tail)
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
        let idx_atomic: *const AtomicU16 = (self.avail_virt + 2) as *const AtomicU16;

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

        // println!(
        //     "[virtio] submit_chain({}): avail->idx {} -> {}",
        //     head_index,
        //     old_idx,
        //     old_idx.wrapping_add(1)
        // );

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
        let idx_atomic: *const AtomicU16 = (self.used_virt + 2) as *const AtomicU16;

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

        // println!(
        //     "[virtio] poll_completion: used->idx {} -> {}, id={} len={}",
        //     self.last_used_idx.wrapping_sub(1),
        //     current,
        //     id,
        //     len
        // );

        Some((id, len))
    }
}
