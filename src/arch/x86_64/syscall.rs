//! x86-64 SYSCALL/SYSRET entry and per-task kernel entry stacks.

use crate::frame_allocator;
use crate::memory;
use crate::println;
use crate::syscall::dispatch;
use ::x86_64::structures::paging::PageTableFlags;
use ::x86_64::{PhysAddr, VirtAddr};
use core::arch::naked_asm;
use core::sync::atomic::AtomicU64;
use gabix_kernel::memory::FRAME_SIZE;

const STAR_KERNEL_SELECTOR_SHIFT: u32 = 32;
const STAR_USER_SELECTOR_SHIFT: u32 = 48;
const SELECTOR_PRIVILEGE_MASK: u64 = 3;
const SYSRET_CODE_SELECTOR_BIAS: u64 = 16;
const SYSCALL_STACK_FRAMES: u64 = 16;

// Byte offsets after the entry's 15 pushes, measured from saved RFLAGS.
// Low to high: RFLAGS, RIP, R15..R12, RBX, RBP, R9, R8, R10, RDX, RSI, RDI, RSP.
const SAVED_ARG1_RDI: usize = 13 * 8;
const SAVED_ARG2_RSI: usize = 12 * 8;
const SAVED_ARG3_RDX: usize = 11 * 8;
const SAVED_ARG4_R10: usize = 10 * 8;
const SAVED_ARG5_R8: usize = 9 * 8;
const SAVED_ARG6_R9: usize = 8 * 8;
// Reserving the seventh C argument also restores 16-byte pre-call alignment.
const STACK_ARGUMENT_BYTES: usize = 8;

/// MSR (Model-Specific Register) addresses for the syscall configuration.
/// These are well-known constants documented in the Intel SDM
/// (Software Developer's Manual) Volume 4.
const MSR_EFER: u32 = 0xC0000080; // Extended Feature Enable Register
const MSR_STAR: u32 = 0xC0000081; // System call target CS and SS
const MSR_LSTAR: u32 = 0xC0000082; // System call target RIP (entry point)
const MSR_FMASK: u32 = 0xC0000084; // System call RFLAGS clear mask

/// Bit 0 of EFER: SYSTEM_CALL_EXTENSIONS (SCE). Enables the SYSCALL instruction.
const EFER_SCE: u64 = 1;

/// Bit 9 of RFLAGS: IF (Interrupt Flag). We clear this on syscall entry
/// so the handler runs with interrupts disabled until we explicitly enable.
const RFLAGS_IF: u64 = 0x200;

/// Virtual address where the dedicated syscall stack begins.
/// Picked to be far from everything else (kernel image, heap, task
/// stacks, user code/stack).
pub(crate) const SYSCALL_STACK_BOTTOM: u64 = 0x6000_0000_0000;
/// 64 KiB syscall entry stack (16 allocator frames).
pub(crate) const SYSCALL_STACK_SIZE: u64 = SYSCALL_STACK_FRAMES * FRAME_SIZE;
pub(crate) const SYSCALL_STACK_TOP: u64 = SYSCALL_STACK_BOTTOM + SYSCALL_STACK_SIZE;

/// Saved user state, captured by `syscall_entry` on every syscall.
///
/// These are needed by `sys_fork`: it clones the current process by
/// snapshotting the user-mode register state at the moment the syscall
/// fired, then constructing a child task whose saved state matches.
///
/// The fork snapshot stores return state and six callee-saved registers.
/// Ordinary syscall return also restores the argument registers from the
/// current task's stack. The fork-child path restores the smaller snapshot.
pub(crate) static USER_RIP_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_RFLAGS_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_RSP_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_RBX_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_RBP_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_R12_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_R13_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_R14_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_R15_SAVE: AtomicU64 = AtomicU64::new(0);

/// Kernel RSP to switch to on syscall entry. Set during init to the top
/// of a dedicated 64 KiB stack region with room below for handler frames.
///
/// Updated by `task::yield_now` on every context switch to point at the
/// NEW current task's `entry_stack_top`. This is what makes per-task
/// entry stacks work: syscall_entry reads this value, and TSS.RSP0 is
/// kept in sync for user-mode timer interrupts.
static KERNEL_RSP: AtomicU64 = AtomicU64::new(0);

/// Set the kernel-side RSP that `syscall_entry` switches to. Called by
/// `task::yield_now` when switching to a new task. The new task's
/// `entry_stack_top` becomes the current entry stack.
pub(crate) fn set_kernel_rsp(rsp: u64) {
    KERNEL_RSP.store(rsp, core::sync::atomic::Ordering::SeqCst);
}

/// Initialize the syscall mechanism: allocate a dedicated kernel stack,
/// set up the MSRs, and save the kernel stack pointer. Must be called
/// from kernel code, before any user program runs.
pub fn init() {
    // Allocate and map a dedicated 64 KiB stack for syscall entry.
    // Using a fresh stack (instead of the captured mid-stack RSP) means
    // we always have plenty of room for handler frames.
    let mut frames_allocated = 0;
    let mut virt = SYSCALL_STACK_BOTTOM;
    while virt < SYSCALL_STACK_TOP {
        let frame = frame_allocator::allocate().expect("out of frames for syscall stack");
        let phys = PhysAddr::new(frame * FRAME_SIZE);
        // SAFETY: virtual range is fresh (unused), frame is fresh.
        unsafe {
            memory::map_to(
                VirtAddr::new(virt),
                phys,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            )
            .expect("map_to failed for syscall stack");
        }
        frames_allocated += 1;
        virt += FRAME_SIZE;
    }
    // KERNEL_RSP = top of the stack region (stack grows DOWN, so the
    // top is where we start).
    KERNEL_RSP.store(SYSCALL_STACK_TOP, core::sync::atomic::Ordering::SeqCst);

    // Mirror it into TSS.RSP0 so the FIRST timer fire in user mode lands
    // on the syscall stack (not the boot stack captured at gdt::init).
    // Without this, the first preempted user task's saved context.rsp
    // ends up pointing at the boot stack — and the subsequent switch-back
    // resumes on the boot stack, which may have been clobbered by then.
    // After the first yield_now, TSS.RSP0 stays in sync with KERNEL_RSP.
    super::gdt::set_rsp0(SYSCALL_STACK_TOP);

    unsafe {
        // Enable the SYSCALL instruction (EFER.SCE = 1).
        let efer = rdmsr(MSR_EFER);
        wrmsr(MSR_EFER, efer | EFER_SCE);

        // SYSCALL takes kernel CS from STAR[32:47]; kernel SS follows it.
        // SYSRET adds 16 to STAR[48:63] for user CS and 8 for user SS.
        // Derive these bases from the GDT selectors used for initial user entry.
        let kernel_code = super::gdt::GDT.1.kernel_code.0 as u64;
        let user_code = super::gdt::GDT.1.user_code.0 as u64;
        let user_base = (user_code & !SELECTOR_PRIVILEGE_MASK) - SYSRET_CODE_SELECTOR_BIAS;
        let selectors =
            (kernel_code << STAR_KERNEL_SELECTOR_SHIFT) | (user_base << STAR_USER_SELECTOR_SHIFT);
        wrmsr(MSR_STAR, selectors);

        // LSTAR: address of the syscall entry point.
        wrmsr(MSR_LSTAR, syscall_entry as *const () as u64);

        // FMASK: bits to clear from RFLAGS on syscall entry. We clear IF
        // so the entry runs with interrupts disabled.
        wrmsr(MSR_FMASK, RFLAGS_IF);
    }

    println!(
        "[syscall] initialized: dedicated stack 0x{:x}-0x{:x} ({} frames), entry=0x{:x}",
        SYSCALL_STACK_BOTTOM,
        SYSCALL_STACK_TOP,
        frames_allocated,
        syscall_entry as *const () as u64
    );
}

/// Read a 64-bit MSR (Model-Specific Register).
unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    core::arch::asm!(
        "rdmsr",
        out("eax") low,
        out("edx") high,
        in("ecx") msr,
        options(nostack, nomem),
    );
    ((high as u64) << 32) | (low as u64)
}

/// Write a 64-bit value to an MSR.
unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nostack, nomem),
    );
}

/// Naked syscall entry point. The CPU jumps here when user code executes
/// the `syscall` instruction.
///
/// On entry, the CPU has already done some work for us:
///   - Saved user RIP into RCX (so we can return via SYSRET)
///   - Saved user RFLAGS into R11
///   - Loaded CS from STAR[32:47] (we're now ring 0)
///   - Cleared the bits in RFLAGS that FMASK specifies (we cleared IF)
///
/// The assembly selects the kernel stack, saves user state, translates the
/// syscall arguments into the Rust dispatcher’s C calling convention, and
/// restores the saved state after the dispatcher returns.
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    naked_asm!(
        // Step 1: Save the user stack pointer while interrupts are disabled,
        // switch to the current task's kernel entry stack, and copy the saved
        // pointer onto that stack.
        "mov [{user_rsp_save}], rsp",
        "mov rsp, [{kernel_rsp}]",
        "push qword ptr [{user_rsp_save}]",

        // Step 2: Preserve every Linux syscall argument register. Linux's
        // syscall ABI only clobbers RAX, RCX, and R11. User code may reuse
        // RDI, RSI, RDX, R10, R8, and R9 immediately after `syscall`.
        "push rdi",
        "push rsi",
        "push rdx",
        "push r10",
        "push r8",
        "push r9",

        // Step 3: Save the six callee-saved registers in this entry frame.
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // Step 4: Push user state for SYSRET.
        "push rcx",                        // user RIP
        "push r11",                        // user RFLAGS

        // Step 5: Snapshot fork state before reusing RCX and R8 for C arguments.
        // Original arguments 5 and 6 remain in the saved stack frame.
        "mov r8, rcx",
        "mov [{user_rip_save}], r8",
        "mov r8, r11",
        "mov [{user_rflags_save}], r8",
        "mov r8, rbx",
        "mov [{user_rbx_save}], r8",
        "mov r8, rbp",
        "mov [{user_rbp_save}], r8",
        "mov r8, r12",
        "mov [{user_r12_save}], r8",
        "mov r8, r13",
        "mov [{user_r13_save}], r8",
        "mov r8, r14",
        "mov [{user_r14_save}], r8",
        "mov r8, r15",
        "mov [{user_r15_save}], r8",

        // Step 6: Set up the System V AMD64 arguments for
        // syscall_handler(num, arg1, arg2, arg3, arg4, arg5, arg6). The first
        // six values use registers. The seventh value goes on the stack.
        // Offsets refer to the 15 saved words above.
        "mov rsi, [rsp + {arg1}]",             // arg1: saved RDI
        "mov rdx, [rsp + {arg2}]",              // arg2: saved RSI
        "mov rcx, [rsp + {arg3}]",              // arg3: saved RDX
        "mov r8,  [rsp + {arg4}]",              // arg4: saved R10
        "mov r9,  [rsp + {arg5}]",              // arg5: saved R8
        "mov rdi, rax",                     // num: syscall RAX

        // Fifteen pushes leave RSP 8 bytes short of call alignment. This slot
        // both aligns the stack and carries arg6 as the seventh C argument.
        "sub rsp, {stack_arg_bytes}",
        "mov r10, [rsp + {arg6_after_reserve}]",              // arg6: saved R9
        "mov [rsp], r10",

        // Step 7: Enable interrupts in the kernel.
        "sti",

        // Step 8: Call the Rust handler. Return value lands in RAX.
        "call {handler}",

        // Step 9: Disable interrupts before returning to user.
        "cli",

        // Step 10: Remove the stacked seventh argument.
        "add rsp, {stack_arg_bytes}",

        // Step 11: Pop user state. RAX keeps the handler return value.
        "pop r11",                         // user RFLAGS
        "pop rcx",                         // user RIP

        // Step 12: Pop callee-saved registers.
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",

        // Step 13: Restore the syscall argument registers.
        "pop r9",
        "pop r8",
        "pop r10",
        "pop rdx",
        "pop rsi",
        "pop rdi",

        // Step 14: Restore user RSP (pop the value we pushed in step 1).
        "pop rsp",

        // Step 15: Return to user. SYSRET uses RCX=RIP, R11=RFLAGS, RAX=return.
        "sysretq",

        arg1 = const SAVED_ARG1_RDI,
        arg2 = const SAVED_ARG2_RSI,
        arg3 = const SAVED_ARG3_RDX,
        arg4 = const SAVED_ARG4_R10,
        arg5 = const SAVED_ARG5_R8,
        arg6_after_reserve = const SAVED_ARG6_R9 + STACK_ARGUMENT_BYTES,
        stack_arg_bytes = const STACK_ARGUMENT_BYTES,
        kernel_rsp = sym KERNEL_RSP,
        handler = sym dispatch,
        user_rsp_save = sym USER_RSP_SAVE,
        user_rip_save = sym USER_RIP_SAVE,
        user_rflags_save = sym USER_RFLAGS_SAVE,
        user_rbx_save = sym USER_RBX_SAVE,
        user_rbp_save = sym USER_RBP_SAVE,
        user_r12_save = sym USER_R12_SAVE,
        user_r13_save = sym USER_R13_SAVE,
        user_r14_save = sym USER_R14_SAVE,
        user_r15_save = sym USER_R15_SAVE,
    );
}
