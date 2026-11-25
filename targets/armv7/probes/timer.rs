#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_unaligned};

/// Return the system timer before and after delaying by the input number of microseconds.
#[no_mangle]
#[link_section = ".text.probe"]
pub unsafe extern "C" fn probe(input: *const u8, len: usize, output: *mut u8, cap: usize) -> i32 {
    if len != 4 || cap < 8 {
        return -1;
    }
    let duration = input.cast::<u32>().read_unaligned();
    if duration > 1_000_000 {
        return -2;
    }
    let counter = 0x3f003004 as *const u32;
    let start = read_volatile(counter);
    while read_volatile(counter).wrapping_sub(start) < duration {
        core::hint::spin_loop();
    }
    write_unaligned(output.cast::<u32>(), start);
    write_unaligned(output.add(4).cast::<u32>(), read_volatile(counter));
    8
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
