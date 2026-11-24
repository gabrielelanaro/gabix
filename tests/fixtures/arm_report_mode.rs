#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let status: u32;
    unsafe {
        core::arch::asm!("mrs {status}, cpsr", status = out(reg) status);
        // CPSR bits 0..4 report the actual CPU mode. User mode is 0x10.
        core::arch::asm!("svc #0", in("r0") status & 0x1f, in("r7") 60u32, options(noreturn));
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
