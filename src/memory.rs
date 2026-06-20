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
        FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PhysFrame,
        Size4KiB, Translate,
    },
    PhysAddr, VirtAddr,
};

/// Initialize the mapper from the active CR3 and the bootloader's physical
/// memory offset, then run sanity checks (translate + a fresh map).
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
    test_map(&mut mapper);
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

/// Create a brand-new mapping end-to-end: allocate a frame, map an unmapped
/// virtual page to it, flush the TLB, then verify by translating + writing.
fn test_map(mapper: &mut OffsetPageTable) {
    let page = Page::containing_address(VirtAddr::new(0xdead_0000));
    let mut allocator = BitmapFrameAllocator;

    // Confirm the page is currently unmapped (sanity check before mapping).
    let before = mapper.translate_addr(page.start_address());
    println!("[mem] test_map: before mapping, {:?} -> {:?}", page.start_address(), before);

    // Allocate a physical frame and map it.
    let frame = allocator.allocate_frame().expect("out of frames");
    let flags = x86_64::structures::paging::PageTableFlags::PRESENT
        | x86_64::structures::paging::PageTableFlags::WRITABLE;

    // SAFETY: the page was previously unmapped (verified above), so we're not
    // aliasing an existing mapping. The new mapping points at a freshly-
    // allocated frame, not at any in-use memory.
    unsafe {
        mapper
            .map_to(page, frame, flags, &mut allocator)
            .expect("map_to failed")
            .flush();
    }

    let after = mapper.translate_addr(page.start_address());
    println!(
        "[mem] test_map: after mapping,  {:?} -> {:?} (frame {:?})",
        page.start_address(),
        after,
        frame,
    );

    // Write to the freshly-mapped page (would have faulted before).
    unsafe {
        let ptr: *mut u64 = page.start_address().as_mut_ptr();
        ptr.write_volatile(0xDEAD_BEEF_CAFE);
        let read_back = ptr.read_volatile();
        println!(
            "[mem] test_map: wrote 0x{:x}, read back 0x{:x} {}",
            0xDEAD_BEEF_CAFE_u64,
            read_back,
            if read_back == 0xDEAD_BEEF_CAFE { "✓" } else { "✗ MISMATCH" },
        );
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
