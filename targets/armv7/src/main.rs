#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod allocator;
mod console;
#[cfg(feature = "virt")]
mod context;
#[cfg(feature = "virt")]
mod memory;
mod platform;
#[cfg(feature = "virt")]
mod scheduler;
#[cfg(feature = "virt")]
mod syscall;
#[cfg(feature = "virt")]
mod user;
#[cfg(feature = "virt")]
mod virt;

#[cfg(feature = "virt")]
use console::write_line;
use console::Pl011;
use core::alloc::Layout;
#[cfg(feature = "virt")]
use core::arch::asm;
use core::arch::global_asm;
use core::panic::PanicInfo;
#[cfg(feature = "virt")]
use gabix_kernel::device::FrameDevice;
use platform::{PLATFORM, UART_BASE};

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
    svc_entry = sym arm_svc_entry,
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
    svc_entry = sym arm_svc_entry,
);

#[no_mangle]
extern "C" fn arm_kernel_entry() {
    let mut console = Pl011::new(UART_BASE);
    gabix_kernel::announce_boot(&mut console, PLATFORM);
    unsafe {
        allocator::init();
    }

    #[cfg(feature = "virt")]
    unsafe {
        let mut disk_user_entry = None;
        if memory::frame_is_reused() {
            write_line(&mut console, b"gabix arm virt frame reused");
        }
        if memory::enable_identity_mapping() {
            write_line(&mut console, b"gabix arm virt memory mapped");
        }
        let boxed = alloc::boxed::Box::new(40u32);
        let mut values = alloc::vec::Vec::new();
        values.extend_from_slice(&[1u32, 1]);
        if *boxed + values.iter().sum::<u32>() == 42 {
            write_line(&mut console, b"gabix arm virt heap allocated");
        }
        if scheduler::task_resumes() {
            write_line(&mut console, b"gabix arm virt task resumed");
        }
        if let Some(mut block) = virt::block::VirtioBlock::probe() {
            if let Some(sector) = block.try_read_sector(1) {
                if sector.starts_with(b"GABIXARM") {
                    write_line(&mut console, b"gabix arm virt block read");
                }
            }
            if let Some(file) = gabix_kernel::fs::ext2::read_file(&mut block, b"/known-block.bin") {
                if file.starts_with(b"gabix-known-disk-data\n") {
                    write_line(&mut console, b"gabix arm virt file read");
                }
            }
            if let Some(image) = gabix_kernel::fs::ext2::read_file(&mut block, b"/init.elf") {
                disk_user_entry = user::load_elf(&image);
            }
        }
        if let Some(mut network) = virt::network::VirtioNet::probe() {
            let mut frame = [0u8; 29];
            frame[..6].fill(0xff);
            frame[6..12].copy_from_slice(&network.mac_address());
            frame[12..14].copy_from_slice(&0x88b5u16.to_be_bytes());
            frame[14..].copy_from_slice(b"gabix-arm-frame");
            network.transmit(&frame);
        }
        virt::timer::init();
        while virt::timer::ticks() < 2 {
            asm!("wfi");
        }
        write_line(&mut console, b"gabix arm virt timer advanced");

        if let Some(entry) = disk_user_entry.or_else(user::loaded_entry) {
            memory::allow_user_section(entry);
            memory::allow_user_section(user::USER_STACK_TOP - 1);
            user::enter(entry, user::USER_STACK_TOP);
        }
    }
}

#[alloc_error_handler]
fn allocation_error(_: Layout) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
extern "C" fn arm_irq_entry() {
    #[cfg(feature = "virt")]
    unsafe {
        virt::timer::handle_interrupt();
    }
}

#[no_mangle]
#[cfg(feature = "virt")]
extern "C" fn arm_svc_entry(frame: *mut syscall::UserFrame) {
    unsafe {
        syscall::dispatch(&mut *frame);
    }
}

#[no_mangle]
#[cfg(feature = "raspi2b")]
extern "C" fn arm_svc_entry(_: *mut u32) {}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
