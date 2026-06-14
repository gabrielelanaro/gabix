//! Interrupt Descriptor Table (IDT) and exception handlers.

use lazy_static::lazy_static;
use x86_64::structures::idt::InterruptDescriptorTable;

lazy_static! {
    /// The kernel's IDT. Currently empty — we'll add handlers in step 2.
    static ref IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();
}

/// Load our IDT. After this, the CPU consults our table when an
/// exception or interrupt fires. (Currently it has no handlers, so any
/// exception will still triple-fault. Step 2 fixes that.)
pub fn init() {
    IDT.load();
}
