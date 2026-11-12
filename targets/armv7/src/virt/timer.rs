use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

const GIC_DISTRIBUTOR_BASE: usize = 0x0800_0000;
const GIC_CPU_BASE: usize = 0x0801_0000;
const PHYSICAL_TIMER_INTERRUPT: u32 = 30;

const GICD_CTLR: usize = 0x000;
const GICD_ISENABLER0: usize = 0x100;
const GICD_ICPENDR0: usize = 0x280;
const GICC_CTLR: usize = 0x000;
const GICC_PMR: usize = 0x004;
const GICC_IAR: usize = 0x00c;
const GICC_EOIR: usize = 0x010;

static TICKS: AtomicUsize = AtomicUsize::new(0);
static mut INTERVAL: u32 = 0;

unsafe fn write32(address: usize, value: u32) {
    core::ptr::write_volatile(address as *mut u32, value);
}

unsafe fn read32(address: usize) -> u32 {
    core::ptr::read_volatile(address as *const u32)
}

unsafe fn reload() {
    let interval = INTERVAL;
    asm!("mcr p15, 0, {value}, c14, c2, 0", value = in(reg) interval);
    let enable = 1u32;
    asm!("mcr p15, 0, {value}, c14, c2, 1", value = in(reg) enable);
    asm!("isb");
}

pub unsafe fn init() {
    write32(GIC_DISTRIBUTOR_BASE + GICD_CTLR, 0);
    write32(
        GIC_DISTRIBUTOR_BASE + GICD_ICPENDR0,
        1 << PHYSICAL_TIMER_INTERRUPT,
    );
    write32(
        GIC_DISTRIBUTOR_BASE + GICD_ISENABLER0,
        1 << PHYSICAL_TIMER_INTERRUPT,
    );
    write32(GIC_DISTRIBUTOR_BASE + GICD_CTLR, 1);

    write32(GIC_CPU_BASE + GICC_PMR, 0xff);
    write32(GIC_CPU_BASE + GICC_CTLR, 1);

    let frequency: u32;
    asm!("mrc p15, 0, {value}, c14, c0, 0", value = out(reg) frequency);
    INTERVAL = frequency / 100;
    reload();
    asm!("cpsie i", "isb");
}

pub fn ticks() -> usize {
    TICKS.load(Ordering::Acquire)
}

pub unsafe fn handle_interrupt() -> bool {
    let acknowledge = read32(GIC_CPU_BASE + GICC_IAR);
    if acknowledge & 0x3ff == PHYSICAL_TIMER_INTERRUPT {
        TICKS.fetch_add(1, Ordering::Release);
        reload();
    }
    write32(GIC_CPU_BASE + GICC_EOIR, acknowledge);
    acknowledge & 0x3ff == PHYSICAL_TIMER_INTERRUPT
}
