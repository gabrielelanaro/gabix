#![no_std] // Don't link the Rust standard library.
#![no_main] // Don't use the normal Rust entry point chain.

mod vga_buffer;
mod gdt;
mod interrupts;

use core::panic::PanicInfo;

/// Called on panic. We have no stack unwinding, so we just halt.
/// Printing the panic info would risk deadlock if the panic happened
/// while WRITER was held; we'll address that in a later slice.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// The kernel entry point. The bootloader jumps here after switching
/// to long mode and setting up paging.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    gdt::init();
    interrupts::init();
    println!("Hello, World{}", "!");
    println!("Welcome to mini-os: a Linux-like kernel in Rust.");

    loop {}
}
