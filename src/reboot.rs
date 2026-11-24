use x86_64::instructions::port::Port;

// Linux arch/x86/kernel/reboot.c uses this command to pulse reset low;
// include/linux/i8042.h names status bit 1 as the input-buffer-full flag.
const I8042_STATUS_COMMAND_PORT: u16 = 0x64;
const INPUT_BUFFER_FULL: u8 = 1 << 1;
const PULSE_RESET_LOW: u8 = 0xfe;

/// Request reset through the legacy i8042 controller, then halt if still running.
/// Assumes this controller exists. The input-buffer wait has no timeout, and
/// this helper neither flushes storage nor tries another reset method.
pub(crate) fn reboot() -> ! {
    unsafe {
        let mut status = Port::<u8>::new(I8042_STATUS_COMMAND_PORT);
        let mut command = Port::<u8>::new(I8042_STATUS_COMMAND_PORT);
        while status.read() & INPUT_BUFFER_FULL != 0 {
            core::hint::spin_loop();
        }
        command.write(PULSE_RESET_LOW);
    }
    crate::arch::x86_64::halt()
}
