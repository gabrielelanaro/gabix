//! x86-64 task context and register switching.

use core::arch::naked_asm;

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
/// and RSP at the fixed offsets read here.
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn fork_trampoline() {
    naked_asm!(
        "mov rcx, [rsp - 64]",
        "mov r11, [rsp - 72]",
        "mov rax, [rsp - 80]",
        "mov rsp, rax",
        "xor rax, rax",
        "cli",
        "sysretq",
    );
}

/// Save the current task's callee-saved registers and restore another task.
///
/// # Safety
///
/// `old` and `new` must point to valid contexts. `new.rsp` must refer to a
/// stack frame created by a previous switch or by the scheduler's task setup.
/// The caller must prevent concurrent access to both contexts.
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
