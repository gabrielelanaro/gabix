#![no_std]
#![no_main]

use core::panic::PanicInfo;

core::arch::global_asm!(include_str!("pi2_large_bss_probe.S"));

const UART_BASE: usize = 0x3f20_1000;
const UART_FLAGS: usize = UART_BASE + 0x18;
const UART_TX_FULL: u32 = 1 << 5;

#[no_mangle]
static mut LARGE_BSS_PROBE: [u32; 256 * 1024] = [0; 256 * 1024];

#[no_mangle]
extern "C" fn pi2_large_bss_probe_main() -> ! {
    let start = core::ptr::addr_of!(LARGE_BSS_PROBE).cast::<u32>();
    let sample_indexes = [0, 1, 1024, 64 * 1024, 128 * 1024, 256 * 1024 - 1];
    let mut all_zero = true;
    for index in sample_indexes {
        all_zero &= unsafe { core::ptr::read_volatile(start.add(index)) } == 0;
    }
    write_byte(if all_zero { b'H' } else { b'E' });
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
