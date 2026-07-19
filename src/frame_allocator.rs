//! Frame allocator: tracks which 4 KB frames of physical RAM are free.
//!
//! Implemented as a bitmap (one bit per frame). Allocation is a linear
//! scan for the first 0 bit; deallocation flips that bit back to 0.
//! See docs/frame-allocator.html for the design rationale and a side-by-
//! side comparison with the buddy allocator Linux uses.

use crate::irq_mutex::IrqMutex;
use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use lazy_static::lazy_static;

/// Size of one physical frame (same as one virtual page on x86_64).
const FRAME_SIZE: usize = 4096;

/// Maximum number of frames this allocator can track. Sized for 512 MB
/// of physical RAM — bump if QEMU is configured with more.
const MAX_FRAMES: usize = 512 * 1024 * 1024 / FRAME_SIZE;

/// Bitmap storage: one bit per frame, packed 8 to a byte.
const BITMAP_BYTES: usize = MAX_FRAMES / 8;

pub struct FrameAllocator {
    /// One bit per frame. 1 = allocated, 0 = free. Bit N corresponds
    /// to frame N, i.e. physical address N * 4096.
    bitmap: [u8; BITMAP_BYTES],
    /// Highest frame number we know about + 1. Frames >= num_frames
    /// are out of range and never scanned.
    num_frames: usize,
}

impl FrameAllocator {
    /// Construct an empty allocator (every frame considered allocated).
    /// Used by lazy_static before `init` is called.
    const fn empty() -> Self {
        FrameAllocator {
            bitmap: [0xFF; BITMAP_BYTES], // 1 = allocated
            num_frames: 0,
        }
    }

    /// Initialize from the bootloader's memory map.
    ///
    /// Every `Usable` region's frames become free; everything else stays
    /// allocated. This is the only time we look at the memory map — after
    /// this we own the bitmap and don't touch `BootInfo` again.
    pub fn init(&mut self, memory_map: &MemoryMap) {
        // Find the highest *Usable* frame number. We only ever hand out
        // Usable frames, so the scan range only needs to cover them — and
        // ignoring the giant Reserved regions at high addresses (device
        // MMIO at 0xfd00000000+) keeps the bitmap small.
        let mut max_frame: usize = 0;
        for region in memory_map.iter() {
            if region.region_type == MemoryRegionType::Usable {
                let end = region.range.end_frame_number as usize;
                if end > max_frame {
                    max_frame = end;
                }
            }
        }
        assert!(
            max_frame <= MAX_FRAMES,
            "memory map reports {} Usable frames but our bitmap holds only {} (bump MAX_FRAMES)",
            max_frame,
            MAX_FRAMES,
        );
        self.num_frames = max_frame;

        // Reset: treat every frame as allocated by default.
        for byte in self.bitmap.iter_mut() {
            *byte = 0xFF;
        }

        // Clear bits for Usable regions only.
        let mut free = 0usize;
        for region in memory_map.iter() {
            if region.region_type == MemoryRegionType::Usable {
                for frame in
                    region.range.start_frame_number as usize..region.range.end_frame_number as usize
                {
                    self.clear_bit(frame);
                    free += 1;
                }
            }
        }

        crate::println!(
            "[frame] {} total frames, {} free (~{} KB usable)",
            self.num_frames,
            free,
            free * FRAME_SIZE / 1024,
        );
    }

    fn set_bit(&mut self, frame: usize) {
        self.bitmap[frame / 8] |= 1 << (frame % 8);
    }

    fn clear_bit(&mut self, frame: usize) {
        self.bitmap[frame / 8] &= !(1 << (frame % 8));
    }

    fn is_set(&self, frame: usize) -> bool {
        (self.bitmap[frame / 8] >> (frame % 8)) & 1 == 1
    }

    /// Find the first free frame, mark it allocated, return its number.
    /// Returns `None` if we're out of memory.
    pub fn allocate(&mut self) -> Option<u64> {
        for frame in 0..self.num_frames {
            if !self.is_set(frame) {
                self.set_bit(frame);
                return Some(frame as u64);
            }
        }
        None
    }

    /// Return a frame to the pool. No-op if out of range or already free.
    pub fn deallocate(&mut self, frame: u64) {
        let frame = frame as usize;
        if frame >= self.num_frames {
            return;
        }
        self.clear_bit(frame);
    }

    /// Number of frames currently free. Linear scan — cheap for our size.
    pub fn frames_free(&self) -> usize {
        (0..self.num_frames).filter(|&f| !self.is_set(f)).count()
    }
}

lazy_static! {
    /// Global frame allocator. Locked because we mutate the bitmap from
    /// any context (eventually including page-fault handlers).
    pub static ref FRAMES: IrqMutex<FrameAllocator> = IrqMutex::new(FrameAllocator::empty());
}

/// One-time initialization. Call from `_start` once `boot_info` is available.
pub fn init(memory_map: &MemoryMap) {
    FRAMES.lock().init(memory_map);
}

/// Allocate one frame. Returns its frame number, or `None` if out of memory.
pub fn allocate() -> Option<u64> {
    FRAMES.lock().allocate()
}

/// Return a frame to the pool.
pub fn deallocate(frame: u64) {
    FRAMES.lock().deallocate(frame)
}

/// Number of frames currently free.
pub fn frames_free() -> usize {
    FRAMES.lock().frames_free()
}
