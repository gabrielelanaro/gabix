//! x86-64 CPU initialization and interrupt machinery.

pub mod apic;
pub mod boot;
pub mod context;
pub mod gdt;
pub mod interrupts;
pub mod paging;
pub mod pic;
pub mod pit;
pub mod syscall;

use core::sync::atomic::Ordering;

pub fn timer_ticks() -> usize {
    interrupts::TICKS.load(Ordering::Relaxed)
}

pub fn halt() -> ! {
    loop {
        ::x86_64::instructions::hlt();
    }
}
