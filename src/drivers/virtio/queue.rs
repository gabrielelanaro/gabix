use super::*;

// ============================================================
// VirtQueue — the shared-memory ring buffer (Lesson 7)
// ============================================================

/// Bytes occupied by one descriptor entry: addr(8) + len(4) + flags(2) + next(2).
const VIRTQ_DESC_SIZE: usize = 16;

// Descriptor flags (bits in the `flags` field).
const VIRTQ_DESC_F_NEXT: u16 = 0x01;
pub(super) const VIRTQ_DESC_F_WRITE: u16 = 0x02;

/// Layout of one descriptor entry. `repr(C)` guarantees the fields land
/// at the spec-mandated offsets: addr at 0, len at 8, flags at 12, next at 14.
#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
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
