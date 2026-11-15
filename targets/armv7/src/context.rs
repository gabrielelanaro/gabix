use core::arch::naked_asm;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct TaskContext {
    pub sp: u32,
    pub user_sp: u32,
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
#[unsafe(naked)]
pub unsafe extern "C" fn switch(old: *mut TaskContext, new: *const TaskContext) {
    naked_asm!(
        "push {{r4-r11, lr}}",
        "str sp, [r0]",
        // ^ selects the user register bank while we remain in SVC mode.
        "add r2, r0, #4",
        "stmia r2, {{sp, lr}}^",
        "add r2, r1, #4",
        "ldmia r2, {{sp, lr}}^",
        "nop",
        "clrex",
        "ldr sp, [r1]",
        "pop {{r4-r11, lr}}",
        "bx lr",
    );
}
