#![no_std]
#![no_main]

use core::panic::PanicInfo;

core::arch::global_asm!(include_str!("uart_smoke.S"));

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
