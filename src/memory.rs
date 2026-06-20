//! Memory management: page-table mapper.
//!
//! Wraps the `x86_64` crate's `OffsetPageTable`, which gives us a safe API
//! over the 4-level page table walk. Initialized from the bootloader's
//! currently-active tables (read via CR3), so we can call `translate_addr`,
//! `map_to`, and `unmap` on what's already there.
//!
//! For now we only test `translate_addr` to verify the mapper works. The
//! `map_to`/`unmap` path needs our frame allocator wrapped in a trait impl,
//! which comes in slice 2.2b.

use crate::println;
use bootloader::bootinfo::BootInfo;
use x86_64::{
    registers::control::Cr3,
    structures::paging::{OffsetPageTable, PageTable, Translate},
    VirtAddr,
};

/// Initialize the mapper from the active CR3 and the bootloader's physical
/// memory offset, then run a translate sanity check.
///
/// The bootloader (with the `map_physical_memory` feature) maps all of
/// physical RAM at a high virtual offset. We use that mapping to access
/// page-table frames, which live at arbitrary physical addresses.
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

    // Translate a few addresses to verify the mapper works.
    let tests: [(u64, &str); 4] = [
        (0x000B_8000, "VGA buffer"),
        (0x0020_7EC0, "_start (kernel entry, link address)"),
        (0x0000_0000, "address 0 (frame zero, NULL guard)"),
        (0x0000_0000_DEAD_BEEF, "(unmapped — expect None)"),
    ];

    for (vaddr, label) in tests {
        let phys = mapper.translate_addr(VirtAddr::new(vaddr));
        println!("[mem]   virt 0x{:>16x}  {:<32} -> {:?}", vaddr, label, phys);
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
