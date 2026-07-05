//! Global Descriptor Table for our kernel.
//!
//! Slice 3.6.a: added user-mode segments (ring 3) and a TSS entry.
//! Preparation for ring 0 ↔ ring 3 transitions in slice 3.6.b. No
//! behavior change yet — just the data structures in place.

use lazy_static::lazy_static;
use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS, Segment};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/// Our Task State Segment. The CPU reads `privilege_stack_table[0]`
/// (commonly called RSP0) from here when transitioning from ring 3 to
/// ring 0 — e.g., when an interrupt fires while running user code.
///
/// In a full system we'd update RSP0 on every context switch to point
/// at the new task's kernel stack. For now we set it once at boot to
/// wherever main's stack happens to be.
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// All the segment selectors we captured when building the GDT, so we
/// don't have to recompute them later. Kernel uses the kernel_*
/// selectors; user code uses user_* selectors (with RPL=3 added when
/// building the iret frame in slice 3.6.b).
pub struct Selectors {
    pub kernel_code: SegmentSelector,
    pub kernel_data: SegmentSelector,
    pub user_code: SegmentSelector,
    pub user_data: SegmentSelector,
    pub tss: SegmentSelector,
}

lazy_static! {
    /// The kernel's GDT, paired with the selectors returned when we
    /// added each entry. We need the selectors later to reload segment
    /// registers (CS, DS, SS) and to build iret frames for user code.
    pub static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());
        // IMPORTANT: user_data comes BEFORE user_code in the GDT so that
        // the SYSRET math works. SYSRET computes:
        //   CS = STAR[48:63] + 16, with RPL=3 forced
        //   SS = STAR[48:63] + 8,  with RPL=3 forced
        // We set STAR[48:63] = 0x10 in syscall::init, which gives:
        //   CS = 0x20 (= index 4 = USER_CODE) → 0x23 with RPL
        //   SS = 0x18 (= index 3 = USER_DATA) → 0x1B with RPL
        // If we put user_code at index 3 (sel 0x18) and user_data at index 4
        // (sel 0x20) — the "obvious" order — sysretq would set SS=0x1B but
        // land on the user_code descriptor (DPL=3, but type=code). Worse:
        // the previous bug was that STAR[48:63] was 0x08, making SS=0x13
        // land on kernel_data (DPL=0) — sysretq itself was unchecked so
        // user mode ran with a "wrong" cached SS, but the timer's eventual
        // iretq checked DPL ≠ CPL and #GP'd. See slice 4.5 debugging.
        let user_data = gdt.append(Descriptor::user_data_segment());
        let user_code = gdt.append(Descriptor::user_code_segment());
        // SAFETY: TSS is a static, so its address is stable for the
        // lifetime of the program. We're the only writer (init runs
        // single-threaded before any task switching).
        let tss = unsafe { gdt.append(Descriptor::tss_segment(&*(&raw const TSS))) };
        (
            gdt,
            Selectors {
                kernel_code,
                kernel_data,
                user_code,
                user_data,
                tss,
            },
        )
    };
}

/// Install our GDT, set up the TSS with the current kernel stack pointer,
/// reload all segment registers, and load the TSS via `ltr`.
pub fn init() {
    // Read the current kernel stack pointer. We use this as RSP0 —
    // the CPU will switch to this stack when transitioning from ring 3
    // to ring 0. For now this is main's stack; in a full kernel we'd
    // allocate a dedicated stack per task for ring transitions.
    let kernel_rsp: u64;
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) kernel_rsp, options(nomem, nostack));
    }

    set_rsp0(kernel_rsp);

    GDT.0.load();
    unsafe {
        // Data, stack, extra — plain `mov` works for these.
        DS::set_reg(GDT.1.kernel_data);
        SS::set_reg(GDT.1.kernel_data);
        ES::set_reg(GDT.1.kernel_data);
        // FS and GS — null selector (we don't use them yet).
        FS::set_reg(SegmentSelector::NULL);
        GS::set_reg(SegmentSelector::NULL);
        // CS is special: the crate internally uses a far-return trick
        // so the instruction pointer and code segment swap atomically.
        CS::set_reg(GDT.1.kernel_code);
        // Load the TSS. After this, the CPU knows where to find RSP0
        // when transitioning from ring 3 to ring 0.
        load_tss(GDT.1.tss);
    }
}

/// Update TSS.RSP0 — the stack the CPU switches to on any ring-3 → ring-0
/// transition (interrupt firing in user mode, syscall using sysretq's
/// implicit stack swap, etc.).
///
/// **Why this must be updated on every context switch:** when an interrupt
/// fires while the CPU is in user mode, the CPU loads RSP from
/// `TSS.privilege_stack_table[0]` and pushes the interrupt frame there.
/// That stack must belong to whichever task is currently running, so that
/// `yield_now` (called from the timer handler) saves the right RSP into
/// the right task's `context.rsp`. If RSP0 stays at the boot stack, every
/// timer-driven preemption saves a boot-stack pointer into the paused
/// task's context — corrupting it for the next switch back.
///
/// Called from `task::yield_now` after picking the new task, alongside
/// `syscall::set_kernel_rsp` (which is the analogous update for the
/// `syscall` instruction's stack swap).
pub fn set_rsp0(rsp: u64) {
    // SAFETY: single CPU, no concurrent access. The TSS is read by the
    // CPU only on the next ring transition, which is after this function
    // returns. Mutating the static here is safe.
    unsafe {
        (*(&raw mut TSS)).privilege_stack_table[0] = VirtAddr::new(rsp);
    }
}
