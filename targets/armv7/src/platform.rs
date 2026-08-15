use gabix_kernel::Platform;

#[cfg(feature = "virt")]
pub const UART_BASE: usize = 0x0900_0000;

#[cfg(feature = "virt")]
pub const PLATFORM: Platform = Platform::ArmVirt;

#[cfg(feature = "raspi2b")]
pub const UART_BASE: usize = 0x3f20_1000;

#[cfg(feature = "raspi2b")]
pub const PLATFORM: Platform = Platform::RaspberryPi2B;

#[cfg(feature = "raspi2b")]
pub fn reset() -> ! {
    crate::raspi2b::reset::reset()
}

#[cfg(feature = "virt")]
pub fn reset() -> ! {
    unsafe {
        let block = [0x0002_0026u32, 0];
        core::arch::asm!(
            "svc #0x123456",
            in("r0") 0x20u32,
            in("r1") block.as_ptr(),
        );
        loop {
            core::arch::asm!("wfi");
        }
    }
}
