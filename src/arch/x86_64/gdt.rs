//! Global Descriptor Table for our kernel.
//!
//! Supplies kernel/user segment selectors and the Task State Segment (TSS).
//! User interrupts select a kernel stack through TSS.RSP0.

use lazy_static::lazy_static;
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/// Our Task State Segment. The CPU reads `privilege_stack_table[0]`
/// (commonly called RSP0) from here when transitioning from ring 3 to
/// ring 0 — e.g., when an interrupt fires while running user code.
///
/// Initialized with the boot stack, then updated by the scheduler to the
/// selected task's entry stack. SYSCALL selects its stack in software.
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// All the segment selectors we captured when building the GDT, so we
/// don't have to recompute them later. Kernel uses the kernel_*
/// selectors; the user selectors returned by `append` already carry RPL=3.
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
        // SYSRET derives SS from STAR[48:63] + 8 and CS from that value + 16,
        // forcing user privilege (RPL=3). User data must precede user code.
        // With STAR's base 0x10, the resulting selectors are SS=0x1b, CS=0x23.
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
    // Use the boot stack until task setup supplies a dedicated entry stack.
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

/// Select the stack for interrupts that enter ring 0 from ring 3.
/// The scheduler updates this alongside the software SYSCALL entry stack.
/// An interrupt frame must land on the running task's stack so that timer
/// preemption saves a context belonging to that task.
pub fn set_rsp0(rsp: u64) {
    // SAFETY: single CPU, no concurrent access. The TSS is read by the
    // CPU only on the next ring transition, which is after this function
    // returns. Mutating the static here is safe.
    unsafe {
        (*(&raw mut TSS)).privilege_stack_table[0] = VirtAddr::new(rsp);
    }
}
