//! CPU architecture implementations.

pub mod x86_64;

/// Interrupt operations used by architecture-independent kernel code.
pub mod interrupts {
    pub fn are_enabled() -> bool {
        ::x86_64::instructions::interrupts::are_enabled()
    }

    pub fn disable() {
        ::x86_64::instructions::interrupts::disable();
    }

    pub fn enable() {
        ::x86_64::instructions::interrupts::enable();
    }

    pub fn enable_and_hlt() {
        ::x86_64::instructions::interrupts::enable_and_hlt();
    }

    pub fn without_interrupts<F, R>(operation: F) -> R
    where
        F: FnOnce() -> R,
    {
        ::x86_64::instructions::interrupts::without_interrupts(operation)
    }
}

pub fn timer_ticks() -> usize {
    x86_64::timer_ticks()
}
