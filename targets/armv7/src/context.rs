use core::arch::naked_asm;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct TaskContext {
    pub sp: u32,
}

impl TaskContext {
    pub const fn empty() -> Self {
        Self { sp: 0 }
    }
}

/// Save the current ARM callee-saved registers and resume another context.
#[unsafe(naked)]
pub unsafe extern "C" fn switch(old: *mut TaskContext, new: *const TaskContext) {
    naked_asm!(
        "push {{r4-r11, lr}}",
        "str sp, [r0]",
        "ldr sp, [r1]",
        "pop {{r4-r11, lr}}",
        "bx lr",
    );
}
