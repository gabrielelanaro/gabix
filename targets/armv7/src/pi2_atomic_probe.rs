#![no_std]
#![no_main]

// This single-core probe attempts one 0 -> 1 compare/exchange. L means success;
// E means failure or panic. It does not test contention or multi-core ordering.
// The assembly entry arms a five-second watchdog and inherits UART setup.
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU32, Ordering};

core::arch::global_asm!(include_str!("pi2_atomic_probe.S"));

const UART_BASE: usize = 0x3f20_1000;
const UART_FLAGS: usize = UART_BASE + 0x18;
const UART_TX_FULL: u32 = 1 << 5;

static ATOMIC_PROBE: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
extern "C" fn pi2_atomic_probe_main() -> ! {
    let exchanged = ATOMIC_PROBE
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok();
    write_byte(if exchanged { b'L' } else { b'E' });
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
