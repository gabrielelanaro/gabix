//! Memory management: page-table mapper, exposed as a global API.
//!
//! Wraps the `x86_64` crate's `OffsetPageTable` in an `IrqMutex<Option<...>>`
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

use crate::irq_mutex::IrqMutex;
use crate::println;
use bootloader::bootinfo::BootInfo;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
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
    static ref MAPPER: IrqMutex<Option<OffsetPageTable<'static>>> = IrqMutex::new(None);
}

/// The bootloader's physical-memory offset, captured at init. Adding this
/// to any physical address gives a virtual address we can dereference from
/// kernel code (the bootloader maps all of physical RAM there).
///
/// Used by `AddressSpace` operations to convert the PML4's physical address
/// (which we own) into a virtual pointer we can read/write.
static PHYS_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Initialize the mapper from the active CR3 and the bootloader's physical
/// memory offset, populate the global `MAPPER`, run sanity checks, then set
/// up the heap.
pub fn init(boot_info: &'static BootInfo) {
    let phys_offset = VirtAddr::new(boot_info.physical_memory_offset);
    PHYS_OFFSET.store(phys_offset.as_u64(), Ordering::SeqCst);

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

/// Return the kernel's physical-memory offset (set at boot from
/// `boot_info.physical_memory_offset`). The direct map covers all of
/// physical RAM at `phys + offset`.
pub fn phys_offset() -> u64 {
    PHYS_OFFSET.load(Ordering::SeqCst)
}

/// Map an MMIO (Memory-Mapped I/O) physical region into the kernel's
/// virtual address space at `phys_offset() + phys_start`. Idempotent —
/// already-mapped pages are skipped.
///
/// Used for virtio device register regions: the device's BAR points at
/// physical addresses (e.g., 0xFE000000 for virtio-blk's modern transport
/// region in QEMU). The kernel reaches them by translating to a virtual
/// address via the direct map. If the bootloader didn't already map the
/// MMIO region (it usually doesn't — the direct map covers RAM, not
/// devices), this function creates the missing page-table entries.
///
/// Pages are mapped `PRESENT | WRITABLE | NO_CACHE | NO_EXECUTE`:
///   - `NO_CACHE` is load-bearing for MMIO — without it, the CPU might
///     cache device register reads/writes, breaking the device protocol.
///   - `NO_EXECUTE` because MMIO must never be executable.
///
/// Returns the virtual address of the start of the region.
pub fn map_mmio(phys_start: u64, size: u64) -> u64 {
    let offset = phys_offset();
    let virt_start = phys_start + offset;

    let start_virt = VirtAddr::new(virt_start);
    let end_virt = VirtAddr::new(virt_start + size.saturating_sub(1));
    let start_page = Page::<Size4KiB>::containing_address(start_virt);
    let end_page = Page::<Size4KiB>::containing_address(end_virt);

    let aspace = AddressSpace::from_current();
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::NO_EXECUTE;

    let mut current_phys = PhysAddr::new(phys_start);
    for page in Page::range_inclusive(start_page, end_page) {
        // Skip already-mapped pages (idempotency for repeat calls).
        if translate(page.start_address()).is_none() {
            // SAFETY: caller guarantees the virtual range is unused for
            // anything other than the MMIO mapping. We pass a physical
            // address that points at device registers, not RAM.
            unsafe {
                aspace
                    .map_to(page.start_address(), current_phys, flags)
                    .unwrap();
            }
        }
        current_phys += 4096u64;
    }

    virt_start
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

/// Error returned by `AddressSpace::unmap` when the page isn't mapped.
pub type UnmapError = x86_64::structures::paging::mapper::UnmapError;

// ============================================================
// AddressSpace (slice 4.4.a)
// ============================================================

/// A first-class address space: owns a PML4 frame, can be loaded into CR3.
///
/// Until now the kernel had a single implicit address space — whatever the
/// bootloader set up, captured at boot into `MAPPER`. To support `fork`
/// (and per-process memory isolation in general) we need to be able to
/// create new address spaces, populate them, and switch between them.
///
/// Each `AddressSpace` owns exactly one physical frame: the PML4 (the
/// top-level page table). Lower levels of the page-table tree (PDPTs, PDs,
/// PTs) are allocated on demand by `map_to` and are not directly tracked
/// here — that's a memory leak for now, fixable later with proper cleanup
/// on process exit.
pub struct AddressSpace {
    /// The physical frame holding this address space's PML4.
    pml4_frame: PhysFrame,
}

impl AddressSpace {
    /// Create a new, empty address space.
    ///
    /// Steps:
    ///   1. Allocate a fresh 4 KB frame for the PML4.
    ///   2. Zero it (all entries start "not present").
    ///   3. Copy entries 256–511 from the *currently active* PML4 (the
    ///      kernel half). Entries 0–255 (the user half) stay zeroed —
    ///      callers fill them in as needed.
    ///
    /// **Why only entries 256–511?** The kernel is linked at high virtual
    /// addresses (`0xFFFFFFFF80000000` and up, set via `--image-base` in
    /// `.cargo/config.toml`), so all kernel code/data lives in PML4 entries
    /// 256–511. Copying just those shares the kernel subtree cleanly: every
    /// process's PML4 has identical entries 256–511 pointing at the same
    /// kernel PDPT/PD/PT frames. Entries 0–255 are per-process user memory.
    ///
    /// **The standalone-test caveat:** a few kernel mappings still live in
    /// the low half (VGA buffer at `0xb8000`, heap at `0x4444_4444_0000`,
    /// physical-memory offset at `0x180_0000_0000`). These are NOT copied
    /// by `new()` alone — accessing them after `switch_to()` will page-fault.
    /// `deep_clone_from_current()` handles them by walking the low half and
    /// sharing non-user frames. The `[test] AddressSpace create + switch`
    /// test only uses serial output (port I/O, no memory mapping needed),
    /// so it works without those low mappings.
    pub fn new() -> Self {
        let pml4_frame = BitmapFrameAllocator
            .allocate_frame()
            .expect("out of frames for new PML4");

        let pml4_virt = phys_to_virt(pml4_frame.start_address());
        let current_pml4_frame = Cr3::read().0;
        let current_pml4_virt = phys_to_virt(current_pml4_frame.start_address());

        // SAFETY: both frames are mapped via the global physical-memory
        // offset (set at boot). We have exclusive access to the new frame
        // (just allocated). The current PML4 we only read.
        unsafe {
            let new_pml4: *mut PageTable = pml4_virt.as_mut_ptr();
            let current_pml4: *const PageTable = current_pml4_virt.as_ptr();

            // Zero the new PML4.
            core::ptr::write_bytes(new_pml4 as *mut u8, 0, 4096);

            // Copy entries 256–511 (kernel half) from current PML4. Each
            // entry holds the physical address of the next-level page table
            // plus flags — copying the raw entry shares the underlying
            // subtree, which is exactly what we want for kernel memory.
            // Entries 0–255 stay zero (per-process user memory).
            for i in 256..512 {
                (&mut *new_pml4)[i] = (&*current_pml4)[i].clone();
            }
        }

        println!(
            "[mem] new AddressSpace: pml4_frame={} (copied 256 kernel entries 256-511 from current CR3={})",
            pml4_frame.start_address().as_u64() / 4096,
            current_pml4_frame.start_address().as_u64() / 4096,
        );

        AddressSpace { pml4_frame }
    }

    /// Load CR3 with this address space's PML4 frame. Single register write.
    ///
    /// After this call, all virtual-to-physical translations on the CPU use
    /// *this* address space's page table. The previous address space is
    /// unaffected — its PML4 still sits in memory, fully populated.
    ///
    /// Switching CR3 invalidates TLB entries (the CPU's translation cache)
    /// unless PCID (Process Context Identifier) is enabled, which we don't.
    /// So address-space switches have a small performance cost — avoid them
    /// when possible (e.g., skip the switch if the new task shares the
    /// current AS).
    pub fn switch_to(&self) {
        // SAFETY: the frame was properly initialized in `new` and is owned
        // by this AddressSpace. Writing CR3 to a valid PML4 is safe.
        unsafe {
            Cr3::write(
                self.pml4_frame,
                x86_64::registers::control::Cr3Flags::empty(),
            );
        }
    }

    /// Construct a temporary `OffsetPageTable` wrapping this address space's
    /// PML4, then run `f` on it. The mapper only lives for the duration of
    /// the closure — we don't keep it around between calls.
    ///
    /// This is the bridge between "we own a PML4 frame" and "the x86_64
    /// crate's mapper API wants a `&mut PageTable`". We compute the PML4's
    /// virtual address from the global physical-memory offset, cast to a
    /// page-table pointer, wrap in `OffsetPageTable`, and hand it to `f`.
    fn with_mapper<R>(&self, f: impl FnOnce(&mut OffsetPageTable) -> R) -> R {
        let pml4_virt = phys_to_virt(self.pml4_frame.start_address());
        let phys_offset = VirtAddr::new(PHYS_OFFSET.load(Ordering::SeqCst));
        // SAFETY: the frame is owned by this AddressSpace and was properly
        // initialized in `new`. We have exclusive access via `&self`
        // (callers must serialize map_to calls — which they do via the
        // scheduler lock pattern).
        let pml4: &mut PageTable = unsafe { &mut *(pml4_virt.as_mut_ptr()) };
        let mut mapper = unsafe { OffsetPageTable::new(pml4, phys_offset) };
        f(&mut mapper)
    }

    /// Map a virtual page to a physical frame in THIS address space, with
    /// the given flags. Allocates any missing intermediate page tables
    /// (PDPT/PD/PT entries) during the walk.
    ///
    /// SAFETY: caller must guarantee the virtual page is currently unmapped
    /// in this address space.
    pub unsafe fn map_to(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageTableFlags,
    ) -> Result<PhysFrame, MapToError> {
        let page = Page::containing_address(virt);
        let frame = PhysFrame::containing_address(phys);
        let mut allocator = BitmapFrameAllocator;
        self.with_mapper(|m| {
            m.map_to(page, frame, flags, &mut allocator)?.flush();
            Ok(frame)
        })
    }

    /// Unmap a virtual page from THIS address space. The physical frame it
    /// pointed at is not freed — just no longer reachable via this virtual
    /// address. (Caller responsible for freeing the frame if appropriate.)
    ///
    /// Used by `sys_fork`: after `AddressSpace::new()` duplicates all the
    /// parent's entries (including user pages pointing at the parent's
    /// frames), fork needs to unmap those entries and replace them with
    /// mappings to fresh child-owned frames.
    pub fn unmap(&self, virt: VirtAddr) -> Result<(), UnmapError> {
        let page: Page<Size4KiB> = Page::containing_address(virt);
        self.with_mapper(|m| {
            m.unmap(page)?.1.flush();
            Ok(())
        })
    }

    /// The physical frame backing this address space's PML4. Used by the
    /// scheduler to compare address spaces without switching.
    pub fn pml4_frame(&self) -> PhysFrame {
        self.pml4_frame
    }

    /// Wrap the currently-active PML4 as an `AddressSpace`. Does NOT
    /// allocate — just reads CR3 and stashes the frame.
    ///
    /// Used for the bootstrap kernel AS (the one the bootloader set up
    /// before our code ran). We can't call `new()` for that because `new()`
    /// allocates a fresh frame and copies entries — but the kernel AS
    /// already exists, fully populated, sitting in CR3. We just need a
    /// handle to it.
    ///
    /// Also used for kernel threads (worker tasks, idle task): they all
    /// share the kernel AS rather than having their own, and `from_current`
    /// is the way to say "use whatever AS is active right now".
    pub fn from_current() -> Self {
        let (frame, _) = Cr3::read();
        AddressSpace { pml4_frame: frame }
    }

    /// Deep-clone the currently-active address space into `self`.
    ///
    /// Two-phase strategy, exploiting the kernel/user PML4-entry split:
    ///
    /// **Phase 1 — copy entries 256–511 (kernel half) verbatim.** Every
    /// process shares the same kernel mappings, so we don't walk the kernel
    /// subtree. We just copy each PML4 entry as-is, which shares the entire
    /// PDPT/PD/PT frame tree underneath. No allocations, no copies — one
    /// `clone()` per entry, ~256 iterations of work that the old version
    /// did by walking four full levels.
    ///
    /// **Phase 2 — walk entries 0–255 (user half + low kernel mappings).**
    /// At each level (PDPT, PD, PT), allocate fresh frames for the dest so
    /// modifications to the dest's PTs don't affect the source. At the leaf
    /// level, behavior depends on the entry's flags:
    ///   - **USER_ACCESSIBLE entries** (user pages): allocate a fresh frame,
    ///     `memcpy` 4 KB from the source frame, point the dest's PT entry
    ///     at the fresh frame. This gives the dest its own private copy
    ///     of every user page (proper fork isolation).
    ///   - **Non-user entries** (kernel pages that still live in the low
    ///     half — the heap, VGA buffer, physical-memory offset): copy the
    ///     entry as-is, sharing the physical frame. Kernel pages are
    ///     typically read-only or read-mostly, so sharing is safe.
    ///
    /// "Huge page" entries (1 GB at PDPT level, 2 MB at PD level) are
    /// copied as-is (shared) — we don't try to break them up.
    ///
    /// # Why this exists
    ///
    /// Without deep cloning, two processes share their intermediate PT
    /// frames via PML4 entry aliasing. Then `child_as.unmap(virt)` writes
    /// to the shared PT and silently unmaps the page in the parent too.
    /// With deep cloning, each process has its own PT frames for the user
    /// half, so this never happens.
    pub fn deep_clone_from_current(&mut self) {
        // SAFETY: we read the source frames via the bootloader's direct
        // map (covers all of physical RAM). Destination frames are owned
        // by us, freshly allocated, exclusively accessed (we're holding
        // no locks but we're also the only CPU).
        unsafe {
            let (src_pml4_frame, _) = Cr3::read();
            let src_pml4: *const PageTable = phys_to_virt(src_pml4_frame.start_address()).as_ptr();
            let dst_pml4: *mut PageTable =
                phys_to_virt(self.pml4_frame.start_address()).as_mut_ptr();

            // Zero the destination PML4 first.
            core::ptr::write_bytes(dst_pml4 as *mut u8, 0, 4096);

            // Phase 1: copy kernel half (entries 256–511) verbatim. No
            // walking, no allocations — just share the source's kernel
            // subtrees. This is the entire win from linking the kernel at
            // high addresses.
            for i in 256..512 {
                (&mut *dst_pml4)[i] = (&*src_pml4)[i].clone();
            }

            // Phase 2: walk user half (entries 0–255). This is where
            // per-process isolation happens.
            for pml4_idx in 0..256u64 {
                let src_entry = (&*src_pml4)[pml4_idx as usize].clone();
                if !src_entry.flags().contains(PageTableFlags::PRESENT) {
                    continue;
                }
                // PML4 entries don't have a HUGE_PAGE bit (they always
                // point at a PDPT), so no huge-page check at this level.
                let src_pdpt_frame = src_entry.frame().expect("src PML4 entry had no frame");
                let dst_pdpt_frame = BitmapFrameAllocator
                    .allocate_frame()
                    .expect("out of frames for deep_clone PDPT");

                // Set the dest PML4 entry to point at the fresh PDPT,
                // preserving the source entry's flags.
                (&mut *dst_pml4)[pml4_idx as usize].set_frame(dst_pdpt_frame, src_entry.flags());

                let src_pdpt: *const PageTable =
                    phys_to_virt(src_pdpt_frame.start_address()).as_ptr();
                let dst_pdpt: *mut PageTable =
                    phys_to_virt(dst_pdpt_frame.start_address()).as_mut_ptr();
                core::ptr::write_bytes(dst_pdpt as *mut u8, 0, 4096);

                for pdpt_idx in 0..512u64 {
                    let src_entry = (&*src_pdpt)[pdpt_idx as usize].clone();
                    if !src_entry.flags().contains(PageTableFlags::PRESENT) {
                        continue;
                    }
                    // 1 GB huge page: copy as-is, share the huge frame.
                    if src_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                        (&mut *dst_pdpt)[pdpt_idx as usize] = src_entry;
                        continue;
                    }
                    let src_pd_frame = src_entry.frame().expect("src PDPT entry had no frame");
                    let dst_pd_frame = BitmapFrameAllocator
                        .allocate_frame()
                        .expect("out of frames for deep_clone PD");

                    (&mut *dst_pdpt)[pdpt_idx as usize].set_frame(dst_pd_frame, src_entry.flags());

                    let src_pd: *const PageTable =
                        phys_to_virt(src_pd_frame.start_address()).as_ptr();
                    let dst_pd: *mut PageTable =
                        phys_to_virt(dst_pd_frame.start_address()).as_mut_ptr();
                    core::ptr::write_bytes(dst_pd as *mut u8, 0, 4096);

                    for pd_idx in 0..512u64 {
                        let src_entry = (&*src_pd)[pd_idx as usize].clone();
                        if !src_entry.flags().contains(PageTableFlags::PRESENT) {
                            continue;
                        }
                        // 2 MB huge page: copy as-is.
                        if src_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
                            (&mut *dst_pd)[pd_idx as usize] = src_entry;
                            continue;
                        }
                        let src_pt_frame = src_entry.frame().expect("src PD entry had no frame");
                        let dst_pt_frame = BitmapFrameAllocator
                            .allocate_frame()
                            .expect("out of frames for deep_clone PT");

                        (&mut *dst_pd)[pd_idx as usize].set_frame(dst_pt_frame, src_entry.flags());

                        let src_pt: *const PageTable =
                            phys_to_virt(src_pt_frame.start_address()).as_ptr();
                        let dst_pt: *mut PageTable =
                            phys_to_virt(dst_pt_frame.start_address()).as_mut_ptr();
                        core::ptr::write_bytes(dst_pt as *mut u8, 0, 4096);

                        for pt_idx in 0..512u64 {
                            let src_entry = (&*src_pt)[pt_idx as usize].clone();
                            if !src_entry.flags().contains(PageTableFlags::PRESENT) {
                                continue;
                            }
                            if src_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE) {
                                // User page: private copy for the dest.
                                let src_frame_phys =
                                    src_entry.frame().expect("src PT entry had no frame");
                                let dst_frame = BitmapFrameAllocator
                                    .allocate_frame()
                                    .expect("out of frames for deep_clone user page");
                                let src_virt = phys_to_virt(src_frame_phys.start_address());
                                let dst_virt = phys_to_virt(dst_frame.start_address());
                                let src_ptr: *const u8 = src_virt.as_u64() as *const u8;
                                let dst_ptr: *mut u8 = dst_virt.as_u64() as *mut u8;
                                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 4096);
                                (&mut *dst_pt)[pt_idx as usize]
                                    .set_frame(dst_frame, src_entry.flags());
                            } else {
                                // Low-half kernel page (heap, VGA, phys-offset):
                                // share the frame (copy entry as-is).
                                (&mut *dst_pt)[pt_idx as usize] = src_entry;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Convert a physical address to a virtual one by adding the global
/// physical-memory offset (set at boot). Works for any physical address
/// in RAM — the bootloader's mapping covers all of it.
fn phys_to_virt(phys: PhysAddr) -> VirtAddr {
    let offset = PHYS_OFFSET.load(Ordering::SeqCst);
    VirtAddr::new(phys.as_u64() + offset)
}

/// Copy the full 4 KB contents of one physical frame to another, using
/// the bootloader's direct map to read/write physical memory from kernel
/// code. Used by `sys_fork` to duplicate user pages: the parent's frame
/// is the source, a freshly-allocated frame is the destination.
///
/// Doesn't touch any page table — operates purely on physical memory via
/// the direct map. This means we can copy bytes between frames that
/// belong to different address spaces without having to switch CR3.
pub fn copy_frame(src_phys: PhysAddr, dst_phys: PhysAddr) {
    let src = phys_to_virt(src_phys).as_u64() as *const u8;
    let dst = phys_to_virt(dst_phys).as_u64() as *mut u8;
    // SAFETY: both frames are mapped via the direct map (bootloader guarantee).
    // They're distinct frames so nonoverlapping is sound.
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, 4096);
    }
}

/// Write `bytes.len()` bytes from `bytes` into a physical frame, starting
/// at offset 0 of the frame. The frame is zeroed first so any trailing
/// bytes (between `bytes.len()` and 4096) are zero rather than garbage.
///
/// Used by `sys_exec` to load program bytes into a freshly-allocated
/// user code page. Operates via the direct map — no AS switch needed.
///
/// Caller must guarantee `bytes.len() <= 4096`.
pub fn write_to_frame(dst_phys: PhysAddr, bytes: &[u8]) {
    assert!(
        bytes.len() <= 4096,
        "write_to_frame: bytes too long for one frame"
    );
    let dst = phys_to_virt(dst_phys).as_u64() as *mut u8;
    // SAFETY: frame is mapped via the direct map.
    unsafe {
        core::ptr::write_bytes(dst, 0, 4096);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    }
}

/// Zero an entire 4 KB frame via the direct map. Used by the ELF loader
/// (Lesson 16) to ensure freshly-allocated user pages have no stale
/// bytes before the segment's file bytes are written in.
pub fn zero_frame(dst_phys: PhysAddr) {
    let dst = phys_to_virt(dst_phys).as_u64() as *mut u8;
    // SAFETY: frame is mapped via the direct map.
    unsafe {
        core::ptr::write_bytes(dst, 0, 4096);
    }
}

/// Write `bytes.len()` bytes into a physical frame starting at `offset`
/// (0..4096). Does NOT zero the frame first — pair with `zero_frame` if
/// needed. Used by the ELF loader to write the slice of file bytes that
/// lands inside a particular page when the segment's `p_vaddr` isn't
/// page-aligned (or when `p_filesz` doesn't fill the page).
///
/// Caller must guarantee `offset + bytes.len() <= 4096`.
pub fn write_to_frame_at(dst_phys: PhysAddr, offset: usize, bytes: &[u8]) {
    assert!(
        offset.checked_add(bytes.len()).map_or(false, |n| n <= 4096),
        "write_to_frame_at: write out of bounds (offset={}, len={})",
        offset,
        bytes.len()
    );
    let dst = phys_to_virt(dst_phys).as_u64() as *mut u8;
    // SAFETY: frame is mapped via the direct map; bounds checked above.
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst.add(offset), bytes.len());
    }
}

/// Run translate on a handful of known addresses to verify the mapper works.
fn translate_tests() {
    // _start's actual link address comes from the kernel image. Since slice
    // 4.6 we link the kernel at high addresses (--image-base in .cargo/config.toml),
    // so _start lives at a high canonical address (0xFFFFFFFF80000000 region)
    // instead of the old low address near 0x200000.
    extern "C" {
        fn _start();
    }
    let start_addr = _start as u64;

    let tests: [(u64, &str); 4] = [
        (0x000B_8000, "VGA buffer (low half, identity-mapped)"),
        (start_addr, "_start (kernel entry, high half)"),
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
        ALLOCATOR.init(heap_start.as_u64() as *mut u8, HEAP_SIZE as usize);
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
    let mapper = guard
        .as_ref()
        .expect("MAPPER not initialized — call memory::init first");
    f(mapper)
}

/// Like `with_mapper` but for mutable access (needed by `map_to`).
fn with_mapper_mut<R>(f: impl FnOnce(&mut OffsetPageTable) -> R) -> R {
    let mut guard = MAPPER.lock();
    let mapper = guard
        .as_mut()
        .expect("MAPPER not initialized — call memory::init first");
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
        crate::frame_allocator::allocate()
            .map(|frame_number| PhysFrame::containing_address(PhysAddr::new(frame_number * 4096)))
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
