//! Memory management: page-table mapper.
//!
//! Wraps the `x86_64` crate's `OffsetPageTable`, which gives us a safe API
//! over the 4-level page table walk. Initialized from the bootloader's
//! currently-active tables (read via CR3), so we can call `translate_addr`,
//! `map_to`, and `unmap` on what's already there.
//!
//! Slice 2.2b adds `BitmapFrameAllocator` (a trait adapter around our frame
//! allocator) and `test_map`, which creates a brand-new mapping end-to-end.

use crate::println;
use bootloader::bootinfo::BootInfo;
use x86_64::{
    registers::control::Cr3,
    structures::paging::{
        FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame,
        Size4KiB, Translate,
    },
    PhysAddr, VirtAddr,
};

/// Virtual address where our heap begins. Picked to be far from everything
/// else (kernel image, stacks, physical-memory offset mapping).
const HEAP_START: u64 = 0x4444_4444_0000;

/// Heap size in bytes. 100 KB is plenty for early kernel Box/Vec usage.
/// Bump allocator wastes everything it touches, so we'd want to bump this
/// once we switch to a freeing allocator.
const HEAP_SIZE: u64 = 100 * 1024;

/// Initialize the mapper from the active CR3 and the bootloader's physical
/// memory offset, run sanity checks (translate + a fresh map), then set up
/// the heap.
pub fn init(boot_info: &'static BootInfo) {
    let phys_offset = VirtAddr::new(boot_info.physical_memory_offset);
    // SAFETY: the bootloader's physical_memory_offset mapping covers all of
    // physical RAM, so the active PML4 (whose address we read from CR3) is
    // reachable via `phys_offset + cr3_phys`.
    let mut mapper = unsafe { OffsetPageTable::new(level_4_table(phys_offset), phys_offset) };

    println!(
        "[mem] mapper initialized; phys_offset = 0x{:x}",
        phys_offset.as_u64()
    );

    translate_tests(&mapper);
    init_heap(&mut mapper);
}

/// Map the heap region (`HEAP_START` to `HEAP_START + HEAP_SIZE`) to fresh
/// physical frames, then tell our `BumpAllocator` about it.
fn init_heap(mapper: &mut OffsetPageTable) {
    use crate::allocator::ALLOCATOR;

    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_end();
    let start_page = Page::containing_address(heap_start);
    let end_page = Page::containing_address(heap_end - 1u64);

    let mut frame_allocator = BitmapFrameAllocator;

    println!(
        "[mem] mapping heap: {:?} - {:?} ({} pages)",
        heap_start,
        heap_end,
        Page::range_inclusive(start_page, end_page).count(),
    );

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = frame_allocator
            .allocate_frame()
            .expect("out of frames while mapping heap");
        // SAFETY: the page is currently unmapped (we just picked this range
        // out of thin air) and the frame is freshly allocated (not in use).
        unsafe {
            mapper
                .map_to(
                    page,
                    frame,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                    &mut frame_allocator,
                )
                .expect("map_to failed during heap setup")
                .flush();
        }
    }

    // SAFETY: the heap range is now mapped and writable, and we're not
    // using it for anything else.
    unsafe {
        ALLOCATOR
            .lock()
            .init(heap_start.as_u64() as *mut u8, HEAP_SIZE as usize);
    }
    println!("[mem] heap initialized: {} bytes", HEAP_SIZE);
}

/// Compute the virtual address one past the end of the heap.
fn heap_end() -> VirtAddr {
    VirtAddr::new(HEAP_START) + HEAP_SIZE
}

/// Run translate_addr on a handful of known addresses to verify the mapper.
fn translate_tests(mapper: &OffsetPageTable) {
    let tests: [(u64, &str); 4] = [
        (0x000B_8000, "VGA buffer"),
        (0x0020_7EC0, "_start (kernel entry, link address)"),
        (0x0000_0000, "address 0 (frame zero, NULL guard)"),
        (0x0000_0000_DEAD_BEEF, "(unmapped — expect None)"),
    ];

    println!("[mem] translate tests:");
    for (vaddr, label) in tests {
        let phys = mapper.translate_addr(VirtAddr::new(vaddr));
        println!("[mem]   virt 0x{:>16x}  {:<32} -> {:?}", vaddr, label, phys);
    }
}

/// Adapter that lets the `x86_64` crate's mapper use our bitmap allocator.
///
/// The mapper's `map_to()` takes a `&mut impl FrameAllocator<Size4KiB>` so
/// it can allocate mid-level page tables during the walk. We implement that
/// trait by forwarding to our global `frame_allocator::FRAMES`.
struct BitmapFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for BitmapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        crate::frame_allocator::allocate().map(|frame_number| {
            PhysFrame::containing_address(PhysAddr::new(frame_number * 4096))
        })
    }
}

/// Get a mutable reference to the active PML4 (top-level page table).
///
/// CR3 holds its physical address; we add the bootloader's physical memory
/// offset to get a virtual address we can dereference.
///
/// SAFETY: caller must guarantee `phys_offset` is a valid mapping that covers
/// the PML4's physical address, and that no other code is concurrently
/// mutating this PageTable.
unsafe fn level_4_table(phys_offset: VirtAddr) -> &'static mut PageTable {
    let (frame, _) = Cr3::read();
    let virt = phys_offset + frame.start_address().as_u64();
    let ptr: *mut PageTable = virt.as_mut_ptr();
    &mut *ptr
}
