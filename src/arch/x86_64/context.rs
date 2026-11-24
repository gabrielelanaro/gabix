//! x86-64 task context and register switching.

use core::arch::naked_asm;

/// Stack words consumed by `switch`: six register pops followed by `ret`.
/// Offsets increase from the saved RSP toward higher addresses.
pub(crate) mod saved_stack {
    pub const R15: usize = 0;
    pub const R14: usize = 1;
    pub const R13: usize = 2;
    pub const R12: usize = 3;
    pub const RBP: usize = 4;
    pub const RBX: usize = 5;
    pub const RETURN_ADDRESS: usize = 6;
    pub const WORD_BYTES: u64 = 8;
    pub const SWITCH_BYTES: u64 = (RETURN_ADDRESS as u64 + 1) * WORD_BYTES;
    // After `ret`, a normal Rust entry needs RSP % 16 == 8.
    pub const RUST_ENTRY_GAP_BYTES: u64 = WORD_BYTES;
    // Fork's assembly entry has RSP = stack_top. Extra user state sits below
    // the saved register frame, at these word distances below its start.
    pub const USER_RIP_BELOW_FRAME: usize = 1;
    pub const USER_FLAGS_BELOW_FRAME: usize = 2;
    pub const USER_RSP_BELOW_FRAME: usize = 3;
    pub const FORK_RIP_OFFSET: u64 = SWITCH_BYTES + USER_RIP_BELOW_FRAME as u64 * WORD_BYTES;
    pub const FORK_FLAGS_OFFSET: u64 = SWITCH_BYTES + USER_FLAGS_BELOW_FRAME as u64 * WORD_BYTES;
    pub const FORK_RSP_OFFSET: u64 = SWITCH_BYTES + USER_RSP_BELOW_FRAME as u64 * WORD_BYTES;
}

/// Saved register context for a paused task.
///
/// The callee-saved register values live on the paused task's stack at
/// this address. Resuming the task loads this RSP and restores the values.
#[derive(Debug, Clone, Copy)]
pub struct TaskContext {
    pub rsp: u64,
}

/// Resume a forked child in user mode with a zero return value.
///
/// The scheduler prepares the child's stack with its user RIP, RFLAGS,
/// and RSP at the offsets shared with `saved_stack`. SYSRETQ takes the user
/// instruction address from RCX and flags from R11; RAX is the fork result.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn fork_trampoline() {
    naked_asm!(
        "mov rcx, [rsp - {user_rip}]",
        "mov r11, [rsp - {user_flags}]",
        "mov rax, [rsp - {user_rsp}]",
        "mov rsp, rax",
        "xor rax, rax",
        "cli",
        "sysretq",
        user_rip = const saved_stack::FORK_RIP_OFFSET,
        user_flags = const saved_stack::FORK_FLAGS_OFFSET,
        user_rsp = const saved_stack::FORK_RSP_OFFSET,
    );
}

/// Save the current task's callee-saved registers and restore another task.
///
/// # Safety
///
/// `old` and `new` must point to valid contexts. `new.rsp` must refer to a
/// stack frame created by a previous switch or by the scheduler's task setup.
/// The caller must prevent concurrent access to both contexts.
/// RDI and RSI carry the two context pointers under the C calling convention.
/// Caller-saved registers are handled by the surrounding call/interrupt path.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn switch(old: *mut TaskContext, new: *const TaskContext) {
    naked_asm!(
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rdi], rsp",
        "mov rsp, [rsi]",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "ret",
    );
}
