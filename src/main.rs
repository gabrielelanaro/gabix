#![no_std] // Don't link the Rust standard library.
#![no_main] // Don't use the normal Rust entry point chain.

mod vga_buffer;
mod gdt;
mod interrupts;
mod pic;
mod pit;

use core::panic::PanicInfo;

/// Called on panic. Prints the panic message and location, then halts.
///
/// If we panicked while WRITER was locked (e.g., during a println! call),
/// the spinlock would deadlock. We avoid this by force_unlocking WRITER
/// before printing — we're about to halt anyway, so safety doesn't matter.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        vga_buffer::WRITER.force_unlock();
    }
    println!("{}", info);
    loop {}
}

/// The kernel entry point. The bootloader jumps here after switching
/// to long mode and setting up paging.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("[boot] gdt...");
    gdt::init();
    println!("[boot] idt...");
    interrupts::init();
    println!("[boot] pic...");
    pic::init();
    println!("[boot] pit...");
    pit::set_frequency(100); // 100 Hz = Linux default; handler throttles to ~1 print/sec
    println!("[boot] hello");
    println!("Hello, World{}", "!");

    println!("[boot] enabling interrupts");
    x86_64::instructions::interrupts::enable();
    println!("[boot] entering main loop");

    loop {}
}
