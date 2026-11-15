//! Suspend a kernel call chain by saving its stack pointer and registers.
//! The saved stack contains r4..r11 followed by lr (the return address).
//! For timer preemption, boot.S has already saved the interrupted CPU state
//! on this same task's kernel stack before calling the Rust IRQ handler.

use core::arch::naked_asm;

pub const SAVED_REGISTER_WORDS: usize = 9;
pub const SAVED_RETURN_ADDRESS_WORD: usize = 8;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct TaskContext {
    /// Kernel stack pointer after switch pushes r4..r11 and lr.
    pub sp: u32,
    /// User-mode stack pointer; ARM keeps it in a separate register bank.
    pub user_sp: u32,
    /// User-mode link register, saved alongside the user stack pointer.
    pub user_lr: u32,
}

impl TaskContext {
    pub const fn empty() -> Self {
        Self {
            sp: 0,
            user_sp: 0,
            user_lr: 0,
        }
    }
}

/// Save the current ARM callee-saved registers and resume another context.
/// The C calling convention supplies old in r0 and new in r1. On resumption,
/// bx lr returns to the new context's suspended call to switch.
/// Safety: run in SVC mode with IRQs masked and valid, separate contexts.
#[unsafe(naked)]
pub unsafe extern "C" fn switch(old: *mut TaskContext, new: *const TaskContext) {
    naked_asm!(
        "push {{r4-r11, lr}}",
        "str sp, [r0]",
        // ^ selects the user register bank while we remain in SVC mode.
        "add r2, r0, #{user_bank}",
        "stmia r2, {{sp, lr}}^",
        "add r2, r1, #{user_bank}",
        "ldmia r2, {{sp, lr}}^",
        "nop",
        // Clear the local exclusive-access reservation across the task change.
        "clrex",
        // Subsequent pops now read the incoming task's saved stack.
        "ldr sp, [r1]",
        "pop {{r4-r11, lr}}",
        "bx lr",
        user_bank = const core::mem::offset_of!(TaskContext, user_sp),
    );
}
