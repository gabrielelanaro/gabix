#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;
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
    rust_entry = sym arm_kernel_entry,
);

#[cfg(feature = "raspi2b")]
global_asm!(
    include_str!("boot.S"),
    uart_base = const 0x3f20_1000,
    gpio_base = const 0x3f20_0000,
    configure_gpio = const 1,
    stack_top = const 0x0800_0000,
    rust_entry = sym arm_kernel_entry,
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

#[no_mangle]
extern "C" fn arm_kernel_entry() {
    let mut console = Pl011::new(UART_BASE);
    gabix_kernel::announce_boot(&mut console, PLATFORM);
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
