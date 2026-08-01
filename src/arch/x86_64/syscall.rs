//! x86-64 SYSCALL/SYSRET entry and per-task kernel entry stacks.

use crate::frame_allocator;
use crate::memory;
use crate::println;
use crate::syscall::dispatch;
use ::x86_64::structures::paging::PageTableFlags;
use ::x86_64::{PhysAddr, VirtAddr};
use core::arch::naked_asm;
use core::sync::atomic::AtomicU64;

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
/// 16 KB syscall stack (4 frames) — matches Linux's kernel stack size.
pub(crate) const SYSCALL_STACK_SIZE: u64 = 16 * 4096;
pub(crate) const SYSCALL_STACK_TOP: u64 = SYSCALL_STACK_BOTTOM + SYSCALL_STACK_SIZE;

/// Saved user state, captured by `syscall_entry` on every syscall.
///
/// These are needed by `sys_fork`: it clones the current process by
/// snapshotting the user-mode register state at the moment the syscall
/// fired, then constructing a child task whose saved state matches.
///
/// The alternative — fishing these values out of the parent's kernel
/// stack frames from inside `sys_fork` — would require knowing the exact
/// stack layout at the time `sys_fork` runs, which is fragile. Global
/// atomics are simpler.
///
/// Only the registers `sys_fork` actually needs are saved here:
///   - user RIP (where to resume in user code — was in RCX)
///   - user RSP (where the user stack pointer was)
///   - user RFLAGS (was in R11)
///   - the 6 callee-saved registers (RBX, RBP, R12-R15) — these MUST be
///     preserved across the syscall per the System V AMD64 ABI, so the
///     child needs the parent's values to look like a true copy.
///
/// Caller-saved registers (RAX, RCX, RDX, RSI, RDI, R8, R9, R10, R11) are
/// NOT saved here: the calling convention allows them to be clobbered
/// across a syscall, so the parent's compiler already treats them as
/// garbage after the syscall. The child gets the same treatment.
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
/// of a dedicated 16 KB stack region with room below for handler frames.
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
    // Allocate and map a dedicated 16 KB stack for syscall entry.
    // Using a fresh stack (instead of the captured mid-stack RSP) means
    // we always have plenty of room for handler frames.
    let mut frames_allocated = 0;
    let mut virt = SYSCALL_STACK_BOTTOM;
    while virt < SYSCALL_STACK_TOP {
        let frame = frame_allocator::allocate().expect("out of frames for syscall stack");
        let phys = PhysAddr::new(frame * 4096);
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
        virt += 4096;
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

        // STAR: kernel CS base in bits 32-47, user CS base in bits 48-63.
        // On SYSCALL: CPU loads CS = STAR[32:47], SS = STAR[32:47] + 8.
        // On SYSRET:  CPU loads CS = STAR[48:63] + 16, SS = STAR[48:63] + 8
        //             (both with RPL=3 added on AMD hardware).
        //
        // Our GDT layout (after the fix in slice 4.5):
        //   index 1 (sel 0x08): kernel code
        //   index 2 (sel 0x10): kernel data
        //   index 3 (sel 0x18): USER DATA   ← (swapped from previous order)
        //   index 4 (sel 0x20): USER CODE
        //   index 5 (sel 0x28): TSS
        //
        // With STAR[32:47] = 0x08 and STAR[48:63] = 0x10:
        //   SYSCALL: CS = 0x08, SS = 0x10 ✓
        //   SYSRET:  CS = 0x10 + 16 = 0x20 | RPL=3 = 0x23 (USER CODE) ✓
        //            SS = 0x10 + 8  = 0x18 | RPL=3 = 0x1B (USER DATA) ✓
        //
        // Previously STAR[48:63] was 0x08, which made SYSRET compute
        // SS = 0x10 (kernel data). sysretq itself doesn't check DPL in
        // long mode, so user code ran with cached SS pointing at kernel
        // data — but the next iretq (e.g., timer firing on user mode)
        // checked DPL ≠ CPL and #GP'd. Fixed by swapping USER_CS / USER_DS
        // in the GDT and setting STAR[48:63] = 0x10.
        wrmsr(MSR_STAR, 0x0010_0008_0000_0000);

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
/// We still need to:
///   - Switch from the user stack to the kernel stack
///   - Save callee-saved registers (the Rust handler will preserve them
///     anyway, but we also need RAX preserved across the call, and we
///     don't have a free caller-saved slot — so we save everything we
///     care about)
///   - Enable interrupts (the handler can be preempted)
///   - Call the Rust handler
///   - Restore everything and SYSRET back
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

        // Step 3: Push callee-saved registers (handler might clobber them
        // if it doesn't follow the convention, and we want them back).
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // Step 4: Push user state for SYSRET.
        "push rcx",                        // user RIP
        "push r11",                        // user RFLAGS

        // Step 5: Snapshot user state to globals so sys_fork can read it.
        // All the values we need are still in their original registers
        // (we just pushed copies above). We use R8 as scratch — it's
        // caller-saved, not used for syscall args 1-4, and we don't use
        // args 5-6 in our kernel. Doing this BEFORE step 4 is important:
        // step 4 clobbers RCX (writes the user's arg3 there), so we have
        // to grab user RIP from RCX first.
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
        "mov rsi, [rsp + 104]",             // arg1: saved RDI
        "mov rdx, [rsp + 96]",              // arg2: saved RSI
        "mov rcx, [rsp + 88]",              // arg3: saved RDX
        "mov r8,  [rsp + 80]",              // arg4: saved R10
        "mov r9,  [rsp + 72]",              // arg5: saved R8
        "mov rdi, rax",                     // num: syscall RAX

        // Fifteen pushes leave RSP 8 bytes short of call alignment. This slot
        // both aligns the stack and carries arg6 as the seventh C argument.
        "sub rsp, 8",
        "mov r10, [rsp + 72]",              // arg6: saved R9
        "mov [rsp], r10",

        // Step 7: Enable interrupts in the kernel.
        "sti",

        // Step 8: Call the Rust handler. Return value lands in RAX.
        "call {handler}",

        // Step 9: Disable interrupts before returning to user.
        "cli",

        // Step 10: Remove the stacked seventh argument.
        "add rsp, 8",

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
