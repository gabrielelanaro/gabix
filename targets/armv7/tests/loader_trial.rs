#![no_std]
#![no_main]
// Run a candidate loader at its real firmware address, changing RAM only.
static LOADER: &[u8] = include_bytes!(env!("GABIX_TRIAL_LOADER"));
core::arch::global_asm!(
    ".syntax unified", ".arm", ".section .text.boot, \"ax\"", ".global _start",
    "_start:", "cpsid if", "ldr sp, =0x08000000", "bl {entry}",
    entry = sym trial,
);
extern "C" fn trial() -> ! {
    if LOADER.len() > 256 * 1024 {
        loop {
            core::hint::spin_loop();
        }
    }
    unsafe {
        for (offset, &byte) in LOADER.iter().enumerate() {
            core::ptr::write_volatile((0x8000 + offset) as *mut u8, byte);
        }
        let zero = 0u32;
        core::arch::asm!("dsb sy", "mcr p15, 0, {zero}, c7, c5, 0", "isb", zero=in(reg) zero);
        core::arch::asm!("bx {entry}",entry=in(reg) 0x8000u32,options(noreturn));
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
