//! Interrupt Descriptor Table (IDT) and exception handlers.

use crate::println;
use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

lazy_static! {
    /// The kernel's IDT. Add handlers here as we implement them.
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt
    };
}

/// Load our IDT. After this, the CPU consults our table when an
/// exception or interrupt fires.
pub fn init() {
    IDT.load();
}

/// Handler for the breakpoint exception (#BP, interrupt 3).
/// Triggered by the `int3` instruction. Safe to invoke — the CPU
/// doesn't think anything went wrong, it just calls us.
extern "x86-interrupt" fn breakpoint_handler(_stack_frame: InterruptStackFrame) {
    // Empty for now — testing if removing println! avoids the XMM save
    // prologue that causes alignment faults.
}
