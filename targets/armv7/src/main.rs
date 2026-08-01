#![no_std]
#![no_main]

mod memory;

#[cfg(feature = "virt")]
use core::arch::asm;
use core::arch::global_asm;
use core::panic::PanicInfo;
#[cfg(feature = "virt")]
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "virt")]
use gabix_kernel::memory::FrameAllocator;
use gabix_kernel::{BootConsole, Platform};

#[cfg(all(feature = "virt", feature = "raspi2b"))]
compile_error!("select one ARM platform");

#[cfg(not(any(feature = "virt", feature = "raspi2b")))]
compile_error!("select an ARM platform");

#[cfg(feature = "virt")]
global_asm!(
    include_str!("boot.S"),
    uart_base = const 0x0900_0000,
    gpio_base = const 0,
    configure_gpio = const 0,
    stack_top = const 0x47ff_0000,
    irq_stack_top = const 0x47fe_0000,
    rust_entry = sym arm_kernel_entry,
    irq_entry = sym arm_irq_entry,
);

#[cfg(feature = "raspi2b")]
global_asm!(
    include_str!("boot.S"),
    uart_base = const 0x3f20_1000,
    gpio_base = const 0x3f20_0000,
    configure_gpio = const 1,
    stack_top = const 0x0800_0000,
    irq_stack_top = const 0x07ff_0000,
    rust_entry = sym arm_kernel_entry,
    irq_entry = sym arm_irq_entry,
);

#[cfg(feature = "virt")]
const UART_BASE: usize = 0x0900_0000;

#[cfg(feature = "virt")]
const PLATFORM: Platform = Platform::ArmVirt;

#[cfg(feature = "raspi2b")]
const UART_BASE: usize = 0x3f20_1000;

#[cfg(feature = "raspi2b")]
const PLATFORM: Platform = Platform::RaspberryPi2B;

struct Pl011 {
    base: *mut u32,
}

impl Pl011 {
    const fn new(base: usize) -> Self {
        Self {
            base: base as *mut u32,
        }
    }
}

impl BootConsole for Pl011 {
    fn write_byte(&mut self, byte: u8) {
        unsafe {
            while core::ptr::read_volatile(self.base.add(0x18 / 4)) & 0x20 != 0 {}
            core::ptr::write_volatile(self.base, byte as u32);
        }
    }
}

#[cfg(feature = "virt")]
fn write_line(console: &mut impl BootConsole, line: &[u8]) {
    for &byte in line {
        console.write_byte(byte);
    }
    console.write_byte(b'\n');
}

#[cfg(feature = "virt")]
static mut FRAMES: FrameAllocator = FrameAllocator::empty();

#[cfg(feature = "virt")]
unsafe fn prove_frame_allocation(console: &mut impl BootConsole) {
    let frames = &mut *core::ptr::addr_of_mut!(FRAMES);
    frames.reset(0x4000_0000, 0x4800_0000);
    frames.add_usable_range(0x4010_0000, 0x47f0_0000);

    let first = frames.allocate().expect("ARM virt has usable RAM");
    let second = frames.allocate().expect("ARM virt has a second frame");
    frames.deallocate(first);
    let reused = frames.allocate().expect("released frame is reusable");

    if first == reused && first != second {
        write_line(console, b"gabix arm virt frame reused");
    }
}

#[cfg(feature = "virt")]
mod virt_timer {
    use super::{asm, AtomicUsize, Ordering};

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

    pub unsafe fn handle_interrupt() {
        let acknowledge = read32(GIC_CPU_BASE + GICC_IAR);
        if acknowledge & 0x3ff == PHYSICAL_TIMER_INTERRUPT {
            TICKS.fetch_add(1, Ordering::Release);
            reload();
        }
        write32(GIC_CPU_BASE + GICC_EOIR, acknowledge);
    }
}

#[no_mangle]
extern "C" fn arm_kernel_entry() {
    let mut console = Pl011::new(UART_BASE);
    gabix_kernel::announce_boot(&mut console, PLATFORM);

    #[cfg(feature = "virt")]
    unsafe {
        prove_frame_allocation(&mut console);
        if memory::enable_identity_mapping() {
            write_line(&mut console, b"gabix arm virt memory mapped");
        }
        virt_timer::init();
        while virt_timer::ticks() < 2 {
            asm!("wfi");
        }
        write_line(&mut console, b"gabix arm virt timer advanced");
    }
}

#[no_mangle]
extern "C" fn arm_irq_entry() {
    #[cfg(feature = "virt")]
    unsafe {
        virt_timer::handle_interrupt();
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
