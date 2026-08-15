use x86_64::instructions::port::Port;

/// Reset a PC through the i8042 controller pulse command.
pub(crate) fn reboot() -> ! {
    unsafe {
        let mut status = Port::<u8>::new(0x64);
        let mut command = Port::<u8>::new(0x64);
        while status.read() & 0x02 != 0 {
            core::hint::spin_loop();
        }
        command.write(0xfe);
    }
    crate::arch::x86_64::halt()
}
