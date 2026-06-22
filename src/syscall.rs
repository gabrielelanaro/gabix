//! Syscall mechanism — the fast path for user-to-kernel transitions.
//!
//! Slice 3.6.c: sets up the SYSCALL/SYSRET instructions via MSRs
//! (Model-Specific Registers) and provides a naked-asm entry point
//! that switches to the kernel stack, calls the Rust handler, then
//! returns via SYSRET.
//!
//! Currently dispatches exactly one syscall: SYS_WRITE (write a buffer
//! to the serial port). User programs call it like:
//!   rax = 1 (SYS_WRITE)
//!   rdi = buffer address
//!   rsi = length
//!   syscall

use crate::println;
use crate::serial;
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

/// Syscall numbers. Match what user programs put in RAX.
/// These follow the Linux x86-64 syscall ABI conventions, so a real
/// Linux binary's syscall numbers would (theoretically) line up —
/// modulo not actually having the right kernel-side implementations.
const SYS_WRITE: u64 = 1;
const SYS_GETPID: u64 = 39;
const SYS_EXIT: u64 = 60;

/// Where to save the user RSP while we're in the kernel.
static USER_RSP_SAVE: AtomicU64 = AtomicU64::new(0);

/// Kernel RSP to switch to on syscall entry. Set once during init to
/// wherever the kernel happens to be running at that point — by the
/// time syscall fires, main has jumped to user mode, so this stack is
/// available for syscall use.
static KERNEL_RSP: AtomicU64 = AtomicU64::new(0);

/// Initialize the syscall mechanism: set up the MSRs and save the
/// kernel stack pointer. Must be called from kernel code, before any
/// user program runs.
pub fn init() {
    // Save current RSP as the kernel stack for syscall entry.
    let krsp: u64;
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) krsp, options(nomem, nostack));
    }
    KERNEL_RSP.store(krsp, core::sync::atomic::Ordering::SeqCst);

    unsafe {
        // Enable the SYSCALL instruction (EFER.SCE = 1).
        let efer = rdmsr(MSR_EFER);
        wrmsr(MSR_EFER, efer | EFER_SCE);

        // STAR: kernel CS base in bits 32-47, user CS base in bits 48-63.
        // On SYSCALL: CPU loads CS = STAR[32:47], SS = STAR[32:47] + 8.
        // On SYSRET:  CPU loads CS = STAR[48:63] + 16, SS = STAR[48:63] + 24
        //             (with RPL=3 added).
        // For our GDT: kernel CS = 0x08, kernel SS = 0x10, user CS = 0x18,
        // user SS = 0x20. So STAR[32:47] = 0x08 (kernel) and
        // STAR[48:63] = 0x08 (so +16 = 0x18 user code).
        wrmsr(MSR_STAR, 0x0008_0008_0000_0000);

        // LSTAR: address of the syscall entry point.
        wrmsr(MSR_LSTAR, syscall_entry as *const () as u64);

        // FMASK: bits to clear from RFLAGS on syscall entry. We clear IF
        // so the entry runs with interrupts disabled.
        wrmsr(MSR_FMASK, RFLAGS_IF);
    }

    println!(
        "[syscall] initialized: KERNEL_RSP=0x{:x}, entry=0x{:x}",
        krsp,
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
        // Step 1: Switch to kernel stack. Use r10 as scratch (caller-saved,
        // we don't care about its entry value).
        "mov r10, rsp",                    // save user RSP in r10
        "mov rsp, [{kernel_rsp}]",         // switch to kernel stack
        "push r10",                        // push user RSP onto kernel stack

        // Step 2: Push callee-saved registers (handler might clobber them
        // if it doesn't follow the convention, and we want them back).
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // Step 3: Push user state for SYSRET.
        "push rcx",                        // user RIP
        "push r11",                        // user RFLAGS

        // Step 4: Set up System V AMD64 calling convention for the Rust
        // handler. Move args from syscall positions to System V positions.
        "mov rcx, rdx",                    // System V arg4 = rdx (was syscall arg3)
        "mov rdx, rsi",                    // System V arg3 = rsi (was syscall arg2)
        "mov rsi, rdi",                    // System V arg2 = rdi (was syscall arg1)
        "mov rdi, rax",                    // System V arg1 = rax (was syscall number)
        // r8, r9 unchanged (args 5, 6 if we ever need them)

        // Step 5: Realign the stack to 16 bytes for the call. We pushed
        // 9 things (1 + 6 + 2) = 72 bytes, which is 8 mod 16. Subtract
        // 8 more to make it 0 mod 16 for the System V ABI.
        "sub rsp, 8",

        // Step 6: Enable interrupts in the kernel.
        "sti",

        // Step 7: Call the Rust handler. Return value lands in RAX.
        "call {handler}",

        // Step 8: Disable interrupts before returning to user.
        "cli",

        // Step 9: Undo the alignment padding.
        "add rsp, 8",

        // Step 10: Save the return value in r10 (caller-saved, won't be popped).
        "mov r10, rax",

        // Step 11: Pop user state.
        "pop r11",                         // user RFLAGS
        "pop rcx",                         // user RIP

        // Step 12: Pop callee-saved registers.
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",

        // Step 13: Restore user RSP (pop the value we pushed in step 1).
        "pop rsp",

        // Step 14: Restore the syscall return value.
        "mov rax, r10",

        // Step 15: Return to user. SYSRET uses RCX=RIP, R11=RFLAGS, RAX=return.
        "sysretq",

        kernel_rsp = sym KERNEL_RSP,
        handler = sym syscall_handler,
    );
}

extern "C" fn syscall_handler(num: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    // Note: keep prints in here MINIMAL. KERNEL_RSP is set to the
    // current RSP at init() time, and there isn't much room below it
    // (we're somewhere mid-stack by the time main calls init). A 4-arg
    // println! overflowed the stack and triple-faulted. The individual
    // syscall implementations below print their own messages, which is
    // enough. (Future fix: allocate a dedicated syscall stack.)
    match num {
        SYS_WRITE => sys_write(arg1, arg2),
        SYS_GETPID => sys_getpid(),
        SYS_EXIT => sys_exit(arg1),
        _ => u64::MAX,
    }
}

/// SYS_WRITE: write `length` bytes from `buffer_ptr` to the serial port.
///
/// Returns the number of bytes written.
fn sys_write(buffer_ptr: u64, length: u64) -> u64 {
    let buffer = unsafe { core::slice::from_raw_parts(buffer_ptr as *const u8, length as usize) };
    for &byte in buffer {
        serial::SERIAL1.lock().write_byte(byte);
    }
    println!("[syscall] sys_write wrote {} bytes", length);
    length
}

/// SYS_GETPID: return the calling process's ID.
///
/// For now we hardcode 1 because we have a single user program and no
/// process tracking. Once `fork`/`exec` (M4 item 4.3) creates real
/// processes, this returns the actual ID of whichever task made the
/// syscall.
fn sys_getpid() -> u64 {
    let pid = 1;
    println!("[syscall] sys_getpid -> {}", pid);
    pid
}

/// SYS_EXIT: terminate the calling process with the given exit code.
///
/// In a real kernel this would: clean up the process's resources,
/// remove it from the scheduler, wake up any parent waiting on it,
/// and switch to another task. We don't have any of that yet — we
/// just halt the kernel. (M4 item 4.3 will add real exit semantics.)
fn sys_exit(exit_code: u64) -> u64 {
    println!(
        "[syscall] sys_exit({}) — halting (single-process kernel for now)",
        exit_code
    );
    x86_64::instructions::interrupts::disable();
    loop {
        x86_64::instructions::hlt();
    }
}
