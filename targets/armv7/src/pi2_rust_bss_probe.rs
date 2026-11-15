#![no_std]
#![no_main]

use core::panic::PanicInfo;

core::arch::global_asm!(include_str!("pi2_rust_bss_probe.S"));

const UART_BASE: usize = 0x3f20_1000;
const UART_FLAGS: usize = UART_BASE + 0x18;
const UART_TX_FULL: u32 = 1 << 5;

const PROBE_WORDS: usize = 16;

#[no_mangle]
static mut BSS_PROBE: [u32; PROBE_WORDS] = [0; PROBE_WORDS];

// Z reports that all sixteen probe words read as zero after assembly poisons and clears BSS.
#[no_mangle]
extern "C" fn pi2_rust_bss_probe_main() -> ! {
    let start = core::ptr::addr_of!(BSS_PROBE).cast::<u32>();
    let mut all_zero = true;
    for index in 0..PROBE_WORDS {
        all_zero &= unsafe { core::ptr::read_volatile(start.add(index)) } == 0;
    }
    write_byte(if all_zero { b'Z' } else { b'E' });
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
