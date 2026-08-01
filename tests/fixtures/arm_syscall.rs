#![no_std]
#![no_main]

#[used]
#[link_section = ".user_header"]
static USER_HEADER: [u32; 2] = [0x5849_4247, 0x4100_0020];

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let message = b"gabix arm userspace wrote\n";
    unsafe {
        core::arch::asm!(
            "svc #0",
            inout("r0") 1u32 => _,
            in("r1") message.as_ptr(),
            in("r2") message.len(),
            in("r7") 1u32,
        );
        core::arch::asm!(
            "svc #0",
            in("r0") 0u32,
            in("r7") 60u32,
            options(noreturn),
        );
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
