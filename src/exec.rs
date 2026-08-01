//! ELF loader — Lesson 16.
//!
//! Takes a parsed ELF (Lesson 15) and turns it into a runnable user
//! process: allocates a fresh `AddressSpace`, maps each `PT_LOAD`
//! segment with the right permissions, copies file bytes into the
//! new memory, zeros the `.bss` tail, and maps a user stack.
//! Returns an `ExecContext` (entry point + stack top + the new AS)
//! to the caller, which then switches CR3 and `iretq`s to user mode.
//!
//! This is the second-to-last piece of the mission. The kernel's
//! Linux counterpart is `load_elf_binary()` in `fs/binfmt_elf.c`.

use crate::elf::{self, PF_W};
use crate::frame_allocator;
use crate::fs;
use crate::memory::{self, AddressSpace};
use crate::println;
use x86_64::structures::paging::PageTableFlags;
use x86_64::{PhysAddr, VirtAddr};

/// Page size on x86-64. The ELF's `p_align` for `PT_LOAD` is usually
/// 4096, matching this — but we don't read `p_align`, we just always
/// page-align.
const PAGE: u64 = 4096;

/// Where to put the user stack. Same address as the existing
/// `user.rs` USER_STACK_TOP — keeps things consistent.
const STACK_TOP: u64 = 0x7001_0000;
/// Stack size: 16 pages = 64 KB. Plenty for hello.elf; real programs
/// would grow this on demand via a page-fault handler.
const STACK_PAGES: u64 = 16;

/// The result of a successful `exec_elf`. The caller uses these to
/// switch to the new address space and jump to user mode.
pub struct ExecContext {
    /// Virtual address the CPU should jump to (`e_entry` from the ELF header).
    pub entry: u64,
    /// Top of the mapped user stack. `rsp` value for the iretq frame.
    pub stack_top: u64,
    /// The new address space. Caller must keep it alive for the
    /// lifetime of the process and call `.switch_to()` before iretq.
    pub aspace: AddressSpace,
}

/// Load + map an ELF from `path`. Returns the entry point, stack top,
/// and the populated address space. Does NOT switch CR3 or jump —
/// the caller does that, so this function can return cleanly on error.
pub fn exec_elf(path: &str) -> Result<ExecContext, &'static str> {
    exec_elf_with(path, false)
}

/// Replace an existing user image with the ELF at `path`.
///
/// The new address space starts as a clone of the current mappings because
/// this kernel still has required low-half kernel mappings. Existing pages at
/// the new ELF segments and fixed user-stack range are removed before the new
/// image is mapped.
pub fn exec_elf_replace(path: &str) -> Result<ExecContext, &'static str> {
    exec_elf_with(path, true)
}

fn exec_elf_with(path: &str, replace_existing: bool) -> Result<ExecContext, &'static str> {
    println!("[exec] loading {}", path);

    // 1. Resolve + read via VFS (Lesson 14 path).
    let inode = fs::resolve_path(path).ok_or("no such file")?;
    let bytes = inode.data.lock();
    println!("[exec]   {} bytes via VFS", bytes.len());

    // 2. Parse + validate (Lesson 15).
    let header = elf::ElfHeader::parse(&bytes).ok_or("not ELF (magic mismatch)")?;
    if !header.is_x86_64_exec() {
        return Err("not a 64-bit LE ET_EXEC x86-64 ELF");
    }
    println!(
        "[exec]   entry=0x{:X}, phnum={}, phentsize={}",
        header.entry, header.phnum, header.phentsize
    );

    // 3. Fresh address space. `new()` copies entries 256–511 (kernel
    //    high half) from current CR3. We also call `deep_clone_from_current`
    //    to share the low-half kernel mappings (heap at 0x4444_4444_0000,
    //    VGA buffer at 0xb8000, physical-memory direct map at
    //    0x180_0000_0000). Without those, any kernel code that touches
    //    heap/VGA/direct-map after switch_to() will page-fault — and
    //    println! touches heap (allocates a String), so it'd crash
    //    immediately.
    let mut aspace = AddressSpace::new();
    aspace.deep_clone_from_current();

    // 4. Walk the PHT, map each PT_LOAD.
    let mut load_count = 0u16;
    for i in 0..header.phnum {
        let off = header.phoff as usize + i as usize * header.phentsize as usize;
        let ph = elf::ProgramHeader::parse(&bytes[off..]).ok_or("ph parse failed")?;
        if ph.is_load() {
            map_segment(&aspace, &ph, &bytes, replace_existing)?;
            load_count += 1;
        }
    }
    if load_count == 0 {
        return Err("no PT_LOAD segments");
    }
    println!("[exec]   mapped {} PT_LOAD segment(s)", load_count);

    // 5. User stack.
    let stack_top = setup_user_stack(&aspace, replace_existing)?;
    println!(
        "[exec]   user stack at 0x{:X} ({} pages)",
        stack_top, STACK_PAGES
    );

    Ok(ExecContext {
        entry: header.entry,
        stack_top,
        aspace,
    })
}

/// Map one `PT_LOAD` segment into `aspace`. For each page in the
/// segment's memory range: allocate a frame, zero it, map it with
/// the segment's permissions, and write the file bytes that fall
/// inside that page (if any).
///
/// The "zero first, then write" pattern handles `.bss` automatically:
/// pages past the file range just stay zeroed.
fn map_segment(
    aspace: &AddressSpace,
    ph: &elf::ProgramHeader,
    file_bytes: &[u8],
    replace_existing: bool,
) -> Result<(), &'static str> {
    assert!(ph.is_load(), "map_segment called on non-PT_LOAD");

    // Page-align the segment's memory range. ELF spec guarantees
    // `p_vaddr ≡ p_offset (mod p_align)`; the segment may start
    // mid-page and end mid-page.
    let page_start = ph.p_vaddr & !(PAGE - 1);
    let page_end = (ph.p_vaddr + ph.p_memsz + PAGE - 1) & !(PAGE - 1);
    let num_pages = (page_end - page_start) / PAGE;

    // Translate p_flags to page-table flags. We always set PRESENT +
    // USER_ACCESSIBLE; WRITABLE conditional on PF_W. NX bit (NO_EXECUTE)
    // requires enabling via EFER.NXE — skipping for now, so 'x' permission
    // is implicit (all mapped pages are executable). That's a security
    // gap (W^X not enforced) but fine for a teaching kernel.
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if ph.p_flags & PF_W != 0 {
        flags |= PageTableFlags::WRITABLE;
    }

    println!(
        "[exec]   PT_LOAD: vaddr=0x{:X} memsz={} filesz={} flags={} ({} pages)",
        ph.p_vaddr,
        ph.p_memsz,
        ph.p_filesz,
        ph.perms_str(),
        num_pages
    );

    for i in 0..num_pages {
        let vaddr = page_start + i * PAGE;
        if replace_existing {
            let _ = aspace.unmap(VirtAddr::new(vaddr));
        }
        let frame_idx = frame_allocator::allocate().ok_or("out of frames during exec")?;
        let phys = PhysAddr::new(frame_idx * PAGE);

        // SAFETY: the virtual page is currently unmapped in the new AS
        // (it's fresh), and the frame is freshly allocated.
        unsafe {
            aspace
                .map_to(VirtAddr::new(vaddr), phys, flags)
                .map_err(|_| "map_to failed")?;
        }

        // Zero first — handles .bss tail automatically.
        memory::zero_frame(phys);

        // Compute the file-byte slice that lands inside this page.
        // File range: [p_offset, p_offset + p_filesz)
        // Memory range for this page: [vaddr, vaddr + PAGE)
        // Memory range of segment: [p_vaddr, p_vaddr + p_filesz)  (for the file part)
        let mem_page_start = vaddr;
        let mem_page_end = vaddr + PAGE;
        let copy_mem_start = mem_page_start.max(ph.p_vaddr);
        let copy_mem_end = mem_page_end.min(ph.p_vaddr + ph.p_filesz);

        if copy_mem_start < copy_mem_end {
            let offset_in_segment = copy_mem_start - ph.p_vaddr;
            let file_off = ph.p_offset + offset_in_segment;
            let len = (copy_mem_end - copy_mem_start) as usize;
            let page_offset = (copy_mem_start - mem_page_start) as usize;
            let slice = &file_bytes[file_off as usize..file_off as usize + len];
            memory::write_to_frame_at(phys, page_offset, slice);
        }
        // else: page is purely .bss (zero) — already zeroed above.
    }

    Ok(())
}

/// Map `STACK_PAGES` rw- user frames ending at `STACK_TOP`.
/// Returns the stack-top virtual address (where `rsp` should point).
fn setup_user_stack(aspace: &AddressSpace, replace_existing: bool) -> Result<u64, &'static str> {
    let flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    for i in 0..STACK_PAGES {
        // i=0 maps the bottom page, i=N-1 maps the top page.
        let vaddr = STACK_TOP - (STACK_PAGES - i) * PAGE;
        if replace_existing {
            let _ = aspace.unmap(VirtAddr::new(vaddr));
        }
        let frame_idx = frame_allocator::allocate().ok_or("out of frames for stack")?;
        let phys = PhysAddr::new(frame_idx * PAGE);
        // SAFETY: fresh virtual range, fresh frame.
        unsafe {
            aspace
                .map_to(VirtAddr::new(vaddr), phys, flags)
                .map_err(|_| "stack map_to failed")?;
        }
        memory::zero_frame(phys);
    }

    Ok(STACK_TOP)
}
