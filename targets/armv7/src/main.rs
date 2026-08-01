#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;

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
    message = sym VIRT_MESSAGE,
);

#[cfg(feature = "raspi2b")]
global_asm!(
    include_str!("boot.S"),
    uart_base = const 0x3f20_1000,
    gpio_base = const 0x3f20_0000,
    configure_gpio = const 1,
    message = sym RASPI2B_MESSAGE,
);

#[cfg(feature = "virt")]
#[used]
#[no_mangle]
static VIRT_MESSAGE: [u8; 23] = *b"gabix arm virt booted\n\0";

#[cfg(feature = "raspi2b")]
#[used]
#[no_mangle]
static RASPI2B_MESSAGE: [u8; 22] = *b"gabix raspi2b booted\n\0";

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
