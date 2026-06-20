#![no_std] // Don't link the Rust standard library.
#![no_main] // Don't use the normal Rust entry point chain.

mod vga_buffer;
mod serial;
mod gdt;
mod interrupts;
mod pic;
mod pit;

use bootloader::BootInfo;
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
        serial::SERIAL1.force_unlock();
    }
    println!("{}", info);
    loop {}
}

/// The kernel entry point. The bootloader jumps here after switching
/// to long mode and setting up paging. It passes us a pointer to a
/// `BootInfo` struct in `rdi` (first arg per the System V AMD64 ABI).
#[no_mangle]
pub extern "C" fn _start(boot_info: &'static BootInfo) -> ! {
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

    println!("[boot] memory map:");
    for region in boot_info.memory_map.iter() {
        // Each line: physical start - physical end (exclusive), and the kind.
        // The "Usable" regions are what we'll later feed to the frame allocator.
        // The bootloader even tags kernel/stack/page-table/bootloader regions
        // separately, so we know which "Usable" frames are actually free.
        println!(
            "  {:>16x} - {:>16x}   {:?}",
            region.range.start_addr(),
            region.range.end_addr(),
            region.region_type,
        );
    }

    println!("[boot] enabling interrupts");
    x86_64::instructions::interrupts::enable();
    println!("[boot] entering main loop");

    loop {}
}
