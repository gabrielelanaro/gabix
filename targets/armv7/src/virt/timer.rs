//! QEMU virt timer delivery: the ARM physical timer raises interrupt 30, and
//! the Generic Interrupt Controller (GIC) delivers it to the CPU's IRQ input.
//! Register names follow the ARM GICv2 and Generic Timer specifications.

use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

const GIC_DISTRIBUTOR_BASE: usize = 0x0800_0000;
const GIC_CPU_BASE: usize = 0x0801_0000;
const PHYSICAL_TIMER_INTERRUPT: u32 = 30;
const INTERRUPT_ID_MASK: u32 = 0x3ff; // GICC_IAR bits 9:0.
const INTERFACE_DISABLED: u32 = 0;
const INTERFACE_ENABLED: u32 = 1;
const PRIORITY_THRESHOLD: u32 = 0xff;
const TIMER_ENABLED_UNMASKED: u32 = 1; // CNTP_CTL: ENABLE=1, IMASK=0.
const TICKS_PER_SECOND: u32 = 100;

const GICD_CTLR: usize = 0x000;
const GICD_ISENABLER0: usize = 0x100;
const GICD_ICPENDR0: usize = 0x280;
const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_IAR: usize = 0x00c;
const GICC_EOIR: usize = 0x010;

static TICKS: AtomicUsize = AtomicUsize::new(0);
static mut COUNTS_PER_TICK: u32 = 0;

unsafe fn write32(address: usize, value: u32) {
    core::ptr::write_volatile(address as *mut u32, value);
}

unsafe fn read32(address: usize) -> u32 {
    core::ptr::read_volatile(address as *const u32)
}

unsafe fn reload() {
    // CNTP_TVAL sets a new deadline relative to the current counter value.
    let interval = COUNTS_PER_TICK;
    asm!("mcr p15, 0, {value}, c14, c2, 0", value = in(reg) interval);
    let enable = TIMER_ENABLED_UNMASKED;
    asm!("mcr p15, 0, {value}, c14, c2, 1", value = in(reg) enable);
    asm!("isb");
}

pub unsafe fn init() {
    // Stop distributor forwarding while clearing and enabling the timer source.
    write32(GIC_DISTRIBUTOR_BASE + GICD_CTLR, INTERFACE_DISABLED);
    write32(
        GIC_DISTRIBUTOR_BASE + GICD_ICPENDR0,
        1 << PHYSICAL_TIMER_INTERRUPT,
    );
    write32(
        GIC_DISTRIBUTOR_BASE + GICD_ISENABLER0,
        1 << PHYSICAL_TIMER_INTERRUPT,
    );
    write32(GIC_DISTRIBUTOR_BASE + GICD_CTLR, INTERFACE_ENABLED);

    write32(GIC_CPU_BASE + GICC_PMR, PRIORITY_THRESHOLD);
    write32(GIC_CPU_BASE + GICC_CTLR, INTERFACE_ENABLED);

    // CNTFRQ supplies counter ticks per second; divide for a 10 ms interval.
    let frequency: u32;
    asm!("mrc p15, 0, {value}, c14, c0, 0", value = out(reg) frequency);
    COUNTS_PER_TICK = frequency / TICKS_PER_SECOND;
    reload();
    asm!("cpsie i", "isb");
}

pub fn ticks() -> usize {
    TICKS.load(Ordering::Acquire)
}

pub unsafe fn handle_interrupt() -> bool {
    // Reading IAR acknowledges the pending interrupt and returns its identity.
    let acknowledge = read32(GIC_CPU_BASE + GICC_IAR);
    if acknowledge & INTERRUPT_ID_MASK == PHYSICAL_TIMER_INTERRUPT {
        TICKS.fetch_add(1, Ordering::Release);
        reload();
    }
    // Return the full acknowledgement value to end this GIC interrupt.
    // The timer has already been rearmed before the scheduler can switch tasks.
    write32(GIC_CPU_BASE + GICC_EOIR, acknowledge);
    acknowledge & INTERRUPT_ID_MASK == PHYSICAL_TIMER_INTERRUPT
}
