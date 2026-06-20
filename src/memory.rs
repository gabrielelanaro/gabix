//! Memory management: page-table mapper, exposed as a global API.
//!
//! Wraps the `x86_64` crate's `OffsetPageTable` in a `Mutex<Option<...>>`
//! behind `lazy_static!`. After `init()` populates it, any module can call
//! `memory::translate(virt)` or `memory::map_to(virt, phys, flags)` without
//! recreating the mapper.
//!
//! History:
//! - Slice 2.2a: created the mapper locally inside `init`, ran translate tests.
//! - Slice 2.2b: added `map_to` end-to-end test.
//! - Slice 2.3/2.3b: added the heap setup (`init_heap`).
//! - Slice 2.5 (this version): extracted the mapper into a global so other
//!   modules can use it. Required by M3 (page-fault handlers, user processes).

use crate::println;
use bootloader::bootinfo::BootInfo;
use lazy_static::lazy_static;
use spin::Mutex;
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
const HEAP_SIZE: u64 = 100 * 1024;

// The kernel's active page-table mapper.
//
// Stored as `Option<...>` because `lazy_static!` can't see `BootInfo` at
// construction time — we fill it in from `init()`. After init, callers can
// assume it's `Some` and lock it to translate or modify mappings.
lazy_static! {
    static ref MAPPER: Mutex<Option<OffsetPageTable<'static>>> = Mutex::new(None);
}

/// Initialize the mapper from the active CR3 and the bootloader's physical
/// memory offset, populate the global `MAPPER`, run sanity checks, then set
/// up the heap.
pub fn init(boot_info: &'static BootInfo) {
    let phys_offset = VirtAddr::new(boot_info.physical_memory_offset);
    // SAFETY: the bootloader's physical_memory_offset mapping covers all of
    // physical RAM, so the active PML4 (whose address we read from CR3) is
    // reachable via `phys_offset + cr3_phys`.
    let mapper = unsafe { OffsetPageTable::new(level_4_table(phys_offset), phys_offset) };

    println!(
        "[mem] mapper initialized; phys_offset = 0x{:x}",
        phys_offset.as_u64()
    );

    // Stash the mapper globally before doing anything else, so init_heap and
    // future callers (eventually page-fault handlers, user-process creation)
    // can reach it via `with_mapper`.
    *MAPPER.lock() = Some(mapper);

    translate_tests();
    init_heap();
}

/// Translate a virtual address to a physical one, returning `None` if the
/// address is unmapped at any level of the page-table walk.
pub fn translate(virt: VirtAddr) -> Option<PhysAddr> {
    with_mapper(|m| m.translate_addr(virt))
}

/// Map a virtual page to a specific physical frame with given flags.
///
/// Allocates any missing intermediate page tables (PML4/PDPT/PD/PT entries)
/// along the walk, drawing the frames from our bitmap allocator.
///
/// SAFETY: caller must guarantee the virtual page is currently unmapped
/// (else this aliases an existing mapping, which is undefined behavior).
pub unsafe fn map_to(
    virt: VirtAddr,
    phys: PhysAddr,
    flags: PageTableFlags,
) -> Result<PhysFrame, MapToError> {
    let page = Page::containing_address(virt);
    let frame = PhysFrame::containing_address(phys);
    let mut allocator = BitmapFrameAllocator;
    // SAFETY: forwarded from caller; same contract.
    with_mapper_mut(|m| {
        // SAFETY: see function-level safety comment.
        m.map_to(page, frame, flags, &mut allocator)?.flush();
        Ok(frame)
    })
}

/// Error returned by `map_to` when the page-table walk can't allocate a
/// needed intermediate table (i.e., out of physical memory).
pub type MapToError = x86_64::structures::paging::mapper::MapToError<Size4KiB>;

/// Run translate on a handful of known addresses to verify the mapper works.
fn translate_tests() {
    let tests: [(u64, &str); 4] = [
        (0x000B_8000, "VGA buffer"),
        (0x0020_7EC0, "_start (kernel entry, link address)"),
        (0x0000_0000, "address 0 (frame zero, NULL guard)"),
        (0x0000_0000_DEAD_BEEF, "(unmapped — expect None)"),
    ];

    println!("[mem] translate tests:");
    for (vaddr, label) in tests {
        let phys = translate(VirtAddr::new(vaddr));
        println!("[mem]   virt 0x{:>16x}  {:<32} -> {:?}", vaddr, label, phys);
    }
}

/// Map the heap region (`HEAP_START` to `HEAP_START + HEAP_SIZE`) to fresh
/// physical frames, then tell our heap allocator about it.
fn init_heap() {
    use crate::allocator::ALLOCATOR;

    let heap_start = VirtAddr::new(HEAP_START);
    let heap_end = heap_end();
    let start_page: Page<Size4KiB> = Page::containing_address(heap_start);
    let end_page: Page<Size4KiB> = Page::containing_address(heap_end - 1u64);

    println!(
        "[mem] mapping heap: {:?} - {:?} ({} pages)",
        heap_start,
        heap_end,
        Page::range_inclusive(start_page, end_page).count(),
    );

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = BitmapFrameAllocator
            .allocate_frame()
            .expect("out of frames while mapping heap");
        // SAFETY: the page is currently unmapped (we just picked this range
        // out of thin air) and the frame is freshly allocated (not in use).
        unsafe {
            map_to(
                page.start_address(),
                frame.start_address(),
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            )
            .expect("map_to failed during heap setup");
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

/// Lock the global MAPPER and run `f` on a shared reference. Panics if the
/// mapper hasn't been initialized yet (i.e., `init()` not called).
fn with_mapper<R>(f: impl FnOnce(&OffsetPageTable) -> R) -> R {
    let guard = MAPPER.lock();
    let mapper = guard.as_ref().expect("MAPPER not initialized — call memory::init first");
    f(mapper)
}

/// Like `with_mapper` but for mutable access (needed by `map_to`).
fn with_mapper_mut<R>(f: impl FnOnce(&mut OffsetPageTable) -> R) -> R {
    let mut guard = MAPPER.lock();
    let mapper = guard.as_mut().expect("MAPPER not initialized — call memory::init first");
    f(mapper)
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
