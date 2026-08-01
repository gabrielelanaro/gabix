//! Architecture-neutral physical-frame allocation.

pub const FRAME_SIZE: u64 = 4096;
const MAX_TRACKED_FRAMES: usize = 1024 * 1024 * 1024 / FRAME_SIZE as usize;
const BITMAP_BYTES: usize = MAX_TRACKED_FRAMES / 8;

/// Bitmap allocator for one contiguous physical-address window.
///
/// Platform code declares which parts of the window are usable. The allocator
/// keeps every other frame reserved, including the kernel image, stacks, and
/// device memory.
pub struct FrameAllocator {
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
        let first = start.div_ceil(FRAME_SIZE);
        let last = end / FRAME_SIZE;
        let count = last.saturating_sub(first) as usize;
        assert!(
            count <= MAX_TRACKED_FRAMES,
            "physical-memory window exceeds the frame bitmap"
        );

        self.base_frame = first;
        self.num_frames = count;
        self.bitmap.fill(0xff);
    }

    /// Mark every complete frame in `[start, end)` as available.
    pub fn add_usable_range(&mut self, start: u64, end: u64) {
        let first = start.div_ceil(FRAME_SIZE).max(self.base_frame);
        let last = (end / FRAME_SIZE).min(self.base_frame + self.num_frames as u64);
        for frame in first..last {
            self.clear_bit((frame - self.base_frame) as usize);
        }
    }

    pub fn allocate(&mut self) -> Option<u64> {
        for index in 0..self.num_frames {
            if !self.is_set(index) {
                self.set_bit(index);
                return Some(self.base_frame + index as u64);
            }
        }
        None
    }

    pub fn deallocate(&mut self, frame: u64) {
        let Some(index) = frame.checked_sub(self.base_frame) else {
            return;
        };
        let index = index as usize;
        if index < self.num_frames {
            self.clear_bit(index);
        }
    }

    pub fn frames_free(&self) -> usize {
        (0..self.num_frames)
            .filter(|&index| !self.is_set(index))
            .count()
    }

    fn set_bit(&mut self, index: usize) {
        self.bitmap[index / 8] |= 1 << (index % 8);
    }

    fn clear_bit(&mut self, index: usize) {
        self.bitmap[index / 8] &= !(1 << (index % 8));
    }

    fn is_set(&self, index: usize) -> bool {
        self.bitmap[index / 8] & (1 << (index % 8)) != 0
    }
}
