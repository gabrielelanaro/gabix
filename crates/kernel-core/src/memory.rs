//! Architecture-neutral physical-frame allocation.

pub const FRAME_SIZE: u64 = 4096;
const MAX_TRACKED_BYTES: usize = 1024 * 1024 * 1024; // One GiB window.
const BITS_PER_BYTE: usize = u8::BITS as usize;
const ALL_FRAMES_UNAVAILABLE: u8 = u8::MAX;
const MAX_TRACKED_FRAMES: usize = MAX_TRACKED_BYTES / FRAME_SIZE as usize;
const BITMAP_BYTES: usize = MAX_TRACKED_FRAMES / BITS_PER_BYTE;

/// Bitmap allocator for one contiguous physical-address window.
///
/// Platform code declares which parts of the window are usable. The allocator
/// keeps every other frame reserved, including the kernel image, stacks, and
/// device memory.
pub struct FrameAllocator {
    // One bit per frame: 1 means reserved or allocated; 0 means available.
    bitmap: [u8; BITMAP_BYTES],
    base_frame: u64,
    num_frames: usize,
}

impl FrameAllocator {
    pub const fn empty() -> Self {
        Self {
            bitmap: [0; BITMAP_BYTES],
            base_frame: 0,
            num_frames: 0,
        }
    }

    /// Reset the allocator to track `[start, end)` with every frame reserved.
    pub fn reset(&mut self, start: u64, end: u64) {
        assert!(start <= end, "physical-memory window is reversed");
        // Round inward: a partial frame at either end must remain unavailable.
        let first = start.div_ceil(FRAME_SIZE);
        let last = end / FRAME_SIZE;
        let count = last.saturating_sub(first) as usize;
        assert!(
            count <= MAX_TRACKED_FRAMES,
            "physical-memory window exceeds the frame bitmap"
        );

        self.base_frame = first;
        self.num_frames = count;
        self.bitmap.fill(ALL_FRAMES_UNAVAILABLE);
    }

    /// Mark every complete frame in `[start, end)` as available.
    pub fn add_usable_range(&mut self, start: u64, end: u64) {
        let first = start.div_ceil(FRAME_SIZE).max(self.base_frame);
        let last = (end / FRAME_SIZE).min(self.base_frame + self.num_frames as u64);
        for frame in first..last {
            self.mark_available((frame - self.base_frame) as usize);
        }
    }

    /// Reserve the first available frame and return its frame number.
    /// Multiply by FRAME_SIZE to obtain its physical byte address.
    pub fn allocate(&mut self) -> Option<u64> {
        for index in 0..self.num_frames {
            if !self.is_unavailable(index) {
                self.mark_unavailable(index);
                return Some(self.base_frame + index as u64);
            }
        }
        None
    }

    /// Release a frame number previously allocated by this instance.
    /// Ownership and double frees are not checked; callers must pass a frame
    /// from this window and keep reserved/in-use frames out of this path.
    pub fn deallocate(&mut self, frame: u64) {
        let Some(index) = frame.checked_sub(self.base_frame) else {
            return;
        };
        let index = index as usize;
        if index < self.num_frames {
            self.mark_available(index);
        }
    }

    pub fn frames_free(&self) -> usize {
        (0..self.num_frames)
            .filter(|&index| !self.is_unavailable(index))
            .count()
    }

    fn mark_unavailable(&mut self, index: usize) {
        self.bitmap[index / BITS_PER_BYTE] |= 1 << (index % BITS_PER_BYTE);
    }

    fn mark_available(&mut self, index: usize) {
        self.bitmap[index / BITS_PER_BYTE] &= !(1 << (index % BITS_PER_BYTE));
    }

    fn is_unavailable(&self, index: usize) -> bool {
        self.bitmap[index / BITS_PER_BYTE] & (1 << (index % BITS_PER_BYTE)) != 0
    }
}
