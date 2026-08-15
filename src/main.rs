#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod allocator;
mod arch;
mod drivers;
mod elf;
mod exec;
mod frame_allocator;
mod fs;
mod irq_mutex;
mod memory;
mod net;
mod process;
mod qemu_exit;
mod reboot;
mod serial;
mod syscall;
mod task;
mod vga_buffer;

use bootloader::BootInfo;
use core::alloc::Layout;
use core::panic::PanicInfo;

#[cfg(feature = "qemu-exit")]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        vga_buffer::WRITER.force_unlock();
        serial::SERIAL1.force_unlock();
    }
    println!("[panic] {}", info);
    qemu_exit::exit_qemu(qemu_exit::EXIT_FAILURE);
}

#[cfg(not(feature = "qemu-exit"))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe {
        vga_buffer::WRITER.force_unlock();
        serial::SERIAL1.force_unlock();
    }
    println!("[panic] {}", info);
    arch::x86_64::halt()
}

#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    panic!(
        "allocation failed: size={}, align={}",
        layout.size(),
        layout.align()
    );
}

#[no_mangle]
pub extern "C" fn _start(boot_info: &'static BootInfo) -> ! {
    arch::x86_64::boot::run(boot_info)
}
