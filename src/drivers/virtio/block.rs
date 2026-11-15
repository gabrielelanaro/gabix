use super::*;
use gabix_kernel::memory::FRAME_SIZE;

const REQUEST_QUEUE: u16 = 0;
const REQUEST_INTERRUPT_ENTRY: u16 = 0;
const MSIX_ADDRESS_HIGH_OFFSET: u64 = 4;
const MSIX_MESSAGE_DATA_OFFSET: u64 = 8;
const MSIX_VECTOR_CONTROL_OFFSET: u64 = 12;
const MSIX_VECTOR_UNMASKED: u32 = 0;
const FEATURE_WORD_LOW: u32 = 0;
const FEATURE_WORD_HIGH: u32 = 1;
const FEATURE_VERSION_1_IN_HIGH_WORD: u32 = (VIRTIO_F_VERSION_1 >> 32) as u32;
const DEVICE_RESET: u8 = 0;
const QUEUE_ENABLED: u16 = 1;
const SECTOR_BYTES: usize = 512;

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
    /// Driver-owned queue storage shared with the device after setup.
    /// Submitted buffers stay reserved until their completion arrives.
    queue: Option<VirtQueue>,
    msix: MsixCap,
}

impl VirtioBlk {
    /// Find the virtio-blk device on the PCI bus, parse its capabilities,
    /// map the common config region into kernel memory, and return a
    /// `VirtioBlk` ready for the handshake.
    ///
    /// Requires a block device with common/notification regions and MSI-X.
    /// Returns `None` if discovery cannot find any required capability.
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

    /// Allocate the request queue and route its completions through MSI-X.
    /// The handle retains the queue for subsequent sector reads.
    pub fn setup_queue(&mut self, queue_size: u16) {
        let queue = VirtQueue::new(queue_size);
        self.configure_queue_at(REQUEST_QUEUE, &queue, "virtio-blk request");
        self.configure_queue_interrupt();
        self.queue = Some(queue);
    }

    fn configure_queue_interrupt(&self) {
        assert!(self.msix.entries > 0);
        let entry = self.msix.table_virt;
        unsafe {
            write_volatile(entry as *mut u32, crate::arch::x86_64::apic::MSI_ADDRESS);
            write_volatile((entry + MSIX_ADDRESS_HIGH_OFFSET) as *mut u32, 0);
            write_volatile(
                (entry + MSIX_MESSAGE_DATA_OFFSET) as *mut u32,
                crate::arch::x86_64::interrupts::VIRTIO_BLK_VECTOR as u32,
            );
            write_volatile(
                (entry + MSIX_VECTOR_CONTROL_OFFSET) as *mut u32,
                MSIX_VECTOR_UNMASKED,
            );
        }
        self.write_common_u16(COMMON_QUEUE_SELECT, REQUEST_QUEUE);
        self.write_common_u16(COMMON_QUEUE_MSIX_VECTOR, REQUEST_INTERRUPT_ENTRY);
        let assigned = self.read_common_u16(COMMON_QUEUE_MSIX_VECTOR);
        assert_eq!(
            assigned, REQUEST_INTERRUPT_ENTRY,
            "virtio-blk rejected MSI-X vector 0"
        );
        println!("[virtio] MSI-X queue 0 -> table 0 -> vector 0x21");
    }

    /// Read one sector synchronously: submit header/data/status descriptors,
    /// then check completion after each interrupt wakeup. Copy the data only
    /// after the device returns the chain and reports success.
    ///
    /// Each call allocates three frames (12 KiB) that are currently retained.
    /// Repeated reads therefore consume memory; buffer reclamation is pending.
    pub fn read_sector(&mut self, sector: u64) -> [u8; SECTOR_BYTES] {
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
                if crate::arch::x86_64::interrupts::VIRTIO_BLK_INTERRUPTS.load(Ordering::Acquire)
                    > 0
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

        let mut block = [0u8; SECTOR_BYTES];
        let data_view = buffers.read_data_prefix(SECTOR_BYTES);
        block.copy_from_slice(&data_view);
        block
    }

    // ---------- Common config region MMIO access ----------

    /// Read 8 bits from the common config region at `offset`.
    fn read_common_u8(&self, offset: u8) -> u8 {
        // SAFETY: the common config region was mapped via `map_mmio` and
        // points at device registers. Volatile read is required so the
        // compiler retains the register access. Queue ordering is handled separately.
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

    /// Reset the device, acknowledge discovery, then identify driver support.
    /// Each status write retains the bits from earlier initialization steps.
    pub fn reset_and_acknowledge(&self) {
        let before = self.read_status();
        println!("[virtio]   device_status before reset: 0x{:02X}", before);

        // Reset
        self.write_status(DEVICE_RESET);
        let after_reset = self.read_status();
        println!(
            "[virtio]   device_status after reset:  0x{:02X}",
            after_reset
        );
        assert_eq!(
            after_reset, DEVICE_RESET,
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

    /// Read both feature words and offer the supported modern-transport bit.
    /// FEATURES_OK readback confirms that the device accepted our selection.
    /// This driver does not request optional block features.
    pub fn negotiate_features(&self) {
        // Read what the device supports, both halves
        self.write_common_u32(COMMON_DEVICE_FEATURE_SELECT, FEATURE_WORD_LOW);
        let device_features_low = self.read_common_u32(COMMON_DEVICE_FEATURE);
        self.write_common_u32(COMMON_DEVICE_FEATURE_SELECT, FEATURE_WORD_HIGH);
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
        let supported_high: u32 = if device_features_high & FEATURE_VERSION_1_IN_HIGH_WORD != 0 {
            FEATURE_VERSION_1_IN_HIGH_WORD
        } else {
            0
        };

        println!("[virtio]   driver features low:  0x{:08X}", supported_low);
        println!(
            "[virtio]   driver features high: 0x{:08X}{}",
            supported_high,
            if supported_high & FEATURE_VERSION_1_IN_HIGH_WORD != 0 {
                " (VIRTIO_F_VERSION_1)"
            } else {
                ""
            }
        );

        // Write the negotiated subset
        self.write_common_u32(COMMON_DRIVER_FEATURE_SELECT, FEATURE_WORD_LOW);
        self.write_common_u32(COMMON_DRIVER_FEATURE, supported_low);
        self.write_common_u32(COMMON_DRIVER_FEATURE_SELECT, FEATURE_WORD_HIGH);
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

    /// Set DRIVER_OK to permit device operation, then check status for errors.
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

    /// Select a queue, set its size and three physical addresses, then enable it.
    /// The caller must keep that storage allocated while the queue is enabled.
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

        // 3-5. Supply the addresses of the shared queue storage.
        self.write_common_u64(COMMON_QUEUE_DESC, queue.desc_phys());
        self.write_common_u64(COMMON_QUEUE_AVAIL, queue.avail_phys());
        self.write_common_u64(COMMON_QUEUE_USED, queue.used_phys());

        // 6. Enable this configured queue.
        self.write_common_u16(COMMON_QUEUE_ENABLE, QUEUE_ENABLED);

        // Verify acceptance — read-back should be 1.
        let enable_readback = self.read_common_u16(COMMON_QUEUE_ENABLE);
        println!(
            "[virtio]   queue_enable readback: {} (expect 1)",
            enable_readback
        );
        assert_eq!(
            enable_readback, QUEUE_ENABLED,
            "device rejected queue configuration — queue_enable didn't stick"
        );
        println!("[virtio] virtqueue {} active", queue_index);
    }

    /// Notify the device that the request queue has available work.
    /// Its register address is notify_base + queue_notify_off * multiplier.
    /// The selected queue's offset and the capability multiplier come from
    /// the device. A zero multiplier uses the notification region's base.
    pub fn kick(&self) {
        // Per-queue offset for this queue, as advertised by the device.
        let queue_notify_off = self.read_common_u16(COMMON_QUEUE_NOTIFY_OFF) as u64;
        let off = queue_notify_off * (self.notify_off_multiplier as u64);
        let notify_addr = self.notify_virt + off;

        // SAFETY: notify_virt was mapped via map_mmio in probe().
        // Writing the queue index (0) is the spec-defined kick action.
        // Volatile so the compiler can't elide the store.
        unsafe {
            write_volatile(notify_addr as *mut u16, REQUEST_QUEUE);
        }
    }
}

impl crate::drivers::BlockDevice for VirtioBlk {
    fn read_sector(&mut self, sector: u64) -> [u8; SECTOR_BYTES] {
        VirtioBlk::read_sector(self, sector)
    }
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

/// Three DMA buffers: request header, sector bytes, and completion status.
/// Each uses one allocator frame so its physical address is directly available.
/// The saved frame numbers support future reclamation; currently there is no
/// Drop implementation, so every request retains its three frames.
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
        let header_phys = header_frame * FRAME_SIZE;
        let data_phys = data_frame * FRAME_SIZE;
        let status_phys = status_frame * FRAME_SIZE;

        let header_virt = header_phys + offset;
        let data_virt = data_phys + offset;
        let status_virt = status_phys + offset;

        // Clear the newly allocated buffers. A zero status also means success;
        // completion is detected through the used ring, never this initial byte.
        // SAFETY: all three frames freshly allocated, reachable via
        // the direct map, no aliases.
        unsafe {
            core::ptr::write_bytes(header_virt as *mut u8, 0, FRAME_SIZE as usize);
            core::ptr::write_bytes(data_virt as *mut u8, 0, FRAME_SIZE as usize);
            core::ptr::write_bytes(status_virt as *mut u8, 0, FRAME_SIZE as usize);
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
        assert!(
            n <= FRAME_SIZE as usize,
            "read_data_prefix asked for too many bytes"
        );
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
