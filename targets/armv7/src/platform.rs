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

/// Exit QEMU through semihosting with status zero. Starting another VM run is
/// the host's responsibility; this adapter does not reboot the virtual machine.
#[cfg(feature = "virt")]
pub fn reset() -> ! {
    const SYS_EXIT_EXTENDED: u32 = 0x20;
    const ADP_STOPPED_APPLICATION_EXIT: u32 = 0x0002_0026;
    const SUCCESS_STATUS: u32 = 0;
    const ARM_SEMIHOSTING_SVC: u32 = 0x123456;
    unsafe {
        let block = [ADP_STOPPED_APPLICATION_EXIT, SUCCESS_STATUS];
        core::arch::asm!(
            "svc #{semihosting_svc}",
            semihosting_svc = const ARM_SEMIHOSTING_SVC,
            in("r0") SYS_EXIT_EXTENDED,
            in("r1") block.as_ptr(),
        );
        loop {
            core::arch::asm!("wfi");
        }
    }
}
