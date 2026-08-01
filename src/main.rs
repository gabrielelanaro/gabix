#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod allocator;
mod apic;
mod boot;
mod elf;
mod exec;
mod ext2;
mod frame_allocator;
mod fs;
mod gdt;
mod interrupts;
mod irq_mutex;
mod memory;
mod net;
mod pci;
mod pic;
mod pit;
mod process;
mod qemu_exit;
mod serial;
mod syscall;
mod task;
mod vga_buffer;
mod virtio;

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
    loop {
        x86_64::instructions::hlt();
    }
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
    boot::run(boot_info)
}
