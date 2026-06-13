#![no_std] // Don't link the Rust standard library.
#![no_main] // Don't use the normal Rust entry point chain.

use core::panic::PanicInfo;

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
    // For now, the kernel just halts. Slice 2 will add booting,
    // slice 3 will add VGA + printing "Hello, World".
    loop {}
}
