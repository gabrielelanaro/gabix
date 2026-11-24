//! Two timer sources: the local timer produces scheduler interrupts, while
//! the system timer counts microseconds for polling deadlines and uptime.
//! Local register fields: BCM2836 ARM-local peripherals, section 4.11.

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

const LOCAL_CONTROL_BASE: usize = 0x4000_0000;
const SYSTEM_TIMER_LOW: *const u32 = 0x3f00_3004 as *const u32;
const SYSTEM_TIMER_HIGH: *const u32 = 0x3f00_3008 as *const u32;
const LOCAL_TIMER_ROUTE: usize = LOCAL_CONTROL_BASE + 0x24;
const LOCAL_TIMER_CONTROL: usize = LOCAL_CONTROL_BASE + 0x34;
const LOCAL_TIMER_ACK: usize = LOCAL_CONTROL_BASE + 0x38;

// The local timer counts both edges of the Pi 2's 19.2 MHz crystal.
const LOCAL_TIMER_COUNTS_PER_SECOND: u32 = 38_400_000;
const TICKS_PER_SECOND: u32 = 100;
const TIMER_RELOAD: u32 = LOCAL_TIMER_COUNTS_PER_SECOND / TICKS_PER_SECOND;
const MICROSECONDS_PER_TICK: u32 = 1_000_000 / TICKS_PER_SECOND;
const MICROSECONDS_PER_MILLISECOND: u64 = 1_000;
const ROUTE_TO_CORE_ZERO_IRQ: u32 = 0;
const TIMER_INTERRUPT_FLAG: u32 = 1 << 31;
const TIMER_RELOAD_NOW: u32 = 1 << 30;
const TIMER_INTERRUPT_ENABLE: u32 = 1 << 29;
const TIMER_ENABLE: u32 = 1 << 28;

static TICKS: AtomicU64 = AtomicU64::new(0);

pub unsafe fn init() {
    write32(LOCAL_TIMER_ROUTE, ROUTE_TO_CORE_ZERO_IRQ);
    write32(LOCAL_TIMER_ACK, TIMER_INTERRUPT_FLAG | TIMER_RELOAD_NOW);
    write32(
        LOCAL_TIMER_CONTROL,
        TIMER_ENABLE | TIMER_INTERRUPT_ENABLE | TIMER_RELOAD,
    );
    // Finish the device writes before allowing IRQ exceptions on this core.
    asm!("dsb sy", "cpsie i", "isb");
}

pub unsafe fn handle_interrupt() {
    // Writing bit 31 clears the pending flag; bit 30 reloads the countdown.
    // Acknowledge the device before counting this interrupt in software.
    write32(LOCAL_TIMER_ACK, TIMER_INTERRUPT_FLAG | TIMER_RELOAD_NOW);
    TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn ticks() -> u64 {
    #[cfg(feature = "pi-network-no-irq-dev")]
    {
        return unsafe { core::ptr::read_volatile(SYSTEM_TIMER_LOW) } as u64
            / MICROSECONDS_PER_TICK as u64;
    }
    #[cfg(not(feature = "pi-network-no-irq-dev"))]
    TICKS.load(Ordering::Relaxed)
}

/// Busy-wait on the system timer. Wrapping subtraction handles a low-word
/// rollover during a short delay; this function does not yield the CPU.
pub fn delay_us(microseconds: u32) {
    let started = unsafe { core::ptr::read_volatile(SYSTEM_TIMER_LOW) };
    while unsafe { core::ptr::read_volatile(SYSTEM_TIMER_LOW) }.wrapping_sub(started) < microseconds
    {
        core::hint::spin_loop();
    }
}

unsafe fn write32(address: usize, value: u32) {
    core::ptr::write_volatile(address as *mut u32, value);
}

/// System timer uptime includes startup before IRQ-driven ticks are enabled.
pub fn uptime_millis() -> u64 {
    unsafe {
        // Retry if the low word rolled over between the two high-word reads.
        loop {
            let high = core::ptr::read_volatile(SYSTEM_TIMER_HIGH);
            let low = core::ptr::read_volatile(SYSTEM_TIMER_LOW);
            if high == core::ptr::read_volatile(SYSTEM_TIMER_HIGH) {
                return (((high as u64) << 32) | low as u64) / MICROSECONDS_PER_MILLISECOND;
            }
        }
    }
}
