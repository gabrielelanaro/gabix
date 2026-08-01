use super::*;

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
            write_volatile(entry as *mut u32, crate::arch::x86_64::apic::MSI_ADDRESS);
            write_volatile((entry + 4) as *mut u32, 0);
            write_volatile(
                (entry + 8) as *mut u32,
                crate::arch::x86_64::interrupts::VIRTIO_BLK_VECTOR as u32,
            );
            write_volatile((entry + 12) as *mut u32, 0);
        }
        self.write_common_u16(COMMON_QUEUE_SELECT, 0);
        self.write_common_u16(COMMON_QUEUE_MSIX_VECTOR, 0);
        let assigned = self.read_common_u16(COMMON_QUEUE_MSIX_VECTOR);
        assert_eq!(assigned, 0, "virtio-blk rejected MSI-X vector 0");
        println!("[virtio] MSI-X queue 0 -> table 0 -> vector 0x21");
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

impl crate::drivers::BlockDevice for VirtioBlk {
    fn read_sector(&mut self, sector: u64) -> [u8; 512] {
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
