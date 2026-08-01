//! x86 boot-memory adapter for the shared physical-frame allocator.

use crate::irq_mutex::IrqMutex;
use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use gabix_kernel::memory::{FrameAllocator, FRAME_SIZE};
use lazy_static::lazy_static;

lazy_static! {
    /// Global frame allocator. Locked because page-fault and task paths can
    /// mutate it while interrupts are enabled.
    pub static ref FRAMES: IrqMutex<FrameAllocator> = IrqMutex::new(FrameAllocator::empty());
}

/// Initialize the shared allocator from the x86 bootloader memory map.
pub fn init(memory_map: &MemoryMap) {
    let max_frame = memory_map
        .iter()
        .filter(|region| region.region_type == MemoryRegionType::Usable)
        .map(|region| region.range.end_frame_number)
        .max()
        .unwrap_or(0);

    let mut frames = FRAMES.lock();
    frames.reset(0, max_frame * FRAME_SIZE);
    for region in memory_map.iter() {
        if region.region_type == MemoryRegionType::Usable {
            frames.add_usable_range(
                region.range.start_frame_number * FRAME_SIZE,
                region.range.end_frame_number * FRAME_SIZE,
            );
        }
    }

    let free = frames.frames_free();
    crate::println!(
        "[frame] {} free frames (~{} KB usable)",
        free,
        free * FRAME_SIZE as usize / 1024,
    );
}

pub fn allocate() -> Option<u64> {
    FRAMES.lock().allocate()
}

pub fn deallocate(frame: u64) {
    FRAMES.lock().deallocate(frame)
}

pub fn frames_free() -> usize {
    FRAMES.lock().frames_free()
}
