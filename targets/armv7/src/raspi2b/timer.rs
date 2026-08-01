use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

const LOCAL_CONTROL_BASE: usize = 0x4000_0000;
const LOCAL_TIMER_ROUTE: usize = LOCAL_CONTROL_BASE + 0x24;
const LOCAL_TIMER_CONTROL: usize = LOCAL_CONTROL_BASE + 0x34;
const LOCAL_TIMER_ACK: usize = LOCAL_CONTROL_BASE + 0x38;

const TIMER_RELOAD: u32 = 384_000;
const TIMER_INTERRUPT_FLAG: u32 = 1 << 31;
const TIMER_RELOAD_NOW: u32 = 1 << 30;
const TIMER_INTERRUPT_ENABLE: u32 = 1 << 29;
const TIMER_ENABLE: u32 = 1 << 28;

static TICKS: AtomicU64 = AtomicU64::new(0);

pub unsafe fn init() {
    write32(LOCAL_TIMER_ROUTE, 0);
    write32(LOCAL_TIMER_ACK, TIMER_INTERRUPT_FLAG | TIMER_RELOAD_NOW);
    write32(
        LOCAL_TIMER_CONTROL,
        TIMER_ENABLE | TIMER_INTERRUPT_ENABLE | TIMER_RELOAD,
    );
    asm!("dsb sy", "cpsie i", "isb");
}

pub unsafe fn handle_interrupt() {
    write32(LOCAL_TIMER_ACK, TIMER_INTERRUPT_FLAG | TIMER_RELOAD_NOW);
    TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

unsafe fn write32(address: usize, value: u32) {
    core::ptr::write_volatile(address as *mut u32, value);
}
