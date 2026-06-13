#![no_std] // Don't link the Rust standard library.
#![no_main] // Don't use the normal Rust entry point chain.

use core::panic::PanicInfo;
use core::ptr;

/// This function is called on panic. We have no std to print or unwind,
/// so the only thing we can do is halt forever.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// The kernel entry point. The bootloader jumps here after switching
/// to long mode and setting up paging. The `#[no_mangle]` attribute
/// prevents Rust from renaming the symbol, so the linker can find it
/// by its exact name `_start`. The `extern "C"` makes the calling
/// convention C-compatible (required by the boot protocol). The
/// `-> !` return type means it never returns.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let vga = 0xb8000 as *mut u8;
    let s = b"Hello, World!";
    for (i, &byte) in s.iter().enumerate() {
        unsafe {
            ptr::write_volatile(vga.add(i * 2), byte);
            ptr::write_volatile(vga.add(i * 2 + 1), 0x0a);
        }
    }
    loop {}
}
