#![no_std]
#![no_main]

mod allocator;
mod memory;

use core::panic::PanicInfo;

core::arch::global_asm!(include_str!("pi2_allocator_init_probe.S"));

const UART_BASE: usize = 0x3f20_1000;
const UART_FLAGS: usize = UART_BASE + 0x18;
const UART_TX_FULL: u32 = 1 << 5;

// S/M/I report SMP-bit readback, identity-map probe and return from allocator
// initialization. This probe makes no allocation.
#[no_mangle]
extern "C" fn pi2_allocator_init_probe_main() -> ! {
    let smp_enabled = unsafe { memory::enable_smp() };
    write_byte(if smp_enabled { b'S' } else { b'E' });
    if !smp_enabled {
        loop {
            core::hint::spin_loop();
        }
    }

    let mapped = unsafe { memory::enable_identity_mapping() };
    write_byte(if mapped { b'M' } else { b'E' });
    if !mapped {
        loop {
            core::hint::spin_loop();
        }
    }

    unsafe {
        allocator::init();
    }
    write_byte(b'I');
    loop {
        core::hint::spin_loop();
    }
}

fn write_byte(byte: u8) {
    unsafe {
        while core::ptr::read_volatile(UART_FLAGS as *const u32) & UART_TX_FULL != 0 {}
        core::ptr::write_volatile(UART_BASE as *mut u32, byte as u32);
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    write_byte(b'E');
    loop {
        core::hint::spin_loop();
    }
}
