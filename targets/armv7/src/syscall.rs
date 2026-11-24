use crate::console::Pl011;
use core::arch::asm;
use gabix_kernel::BootConsole;

const SYS_WRITE: u32 = 1;
const SYS_GETPID: u32 = 39;
const SYS_EXIT: u32 = 60;
const SYS_REBOOT: u32 = 169;

const SYSCALL_NUMBER_REGISTER: usize = 7;
const RESULT_REGISTER: usize = 0;
const SAVED_GENERAL_REGISTERS: usize = 13;
const SYSCALL_ERROR: u32 = u32::MAX;
const STDOUT_FD: u32 = 1;
const MAX_WRITE_BYTES: u32 = 4096;

/// Matches boot.S .Lsvc: r0 through r12, followed by LR_svc.
/// r7 selects the call; r0-r2 carry arguments; the saved r0 becomes the result.
/// The saved processor status remains in SPSR_svc, outside this frame.
#[repr(C)]
pub struct UserFrame {
    registers: [u32; SAVED_GENERAL_REGISTERS],
    return_address: u32,
}

/// Dispatch Gabix's small ARM ABI (its numbers are defined above).
/// Fixed scheduled processes can exit/getpid but cannot reboot the machine.
/// Bootstrap user programs use the platform-specific exit/reboot path.
/// Safety: frame must be the live SVC frame, with valid current-process state.
pub unsafe fn dispatch(frame: &mut UserFrame) {
    if let Some(id) = crate::scheduler::current_process() {
        match frame.registers[SYSCALL_NUMBER_REGISTER] {
            SYS_EXIT => crate::process::exit_current(frame.registers[0]),
            SYS_GETPID => {
                frame.registers[RESULT_REGISTER] = id as u32 + 1;
                return;
            }
            SYS_REBOOT => {
                frame.registers[RESULT_REGISTER] = SYSCALL_ERROR;
                return;
            }
            _ => {}
        }
    }
    #[cfg(all(feature = "pi-wifi", feature = "pi-managed"))]
    if let Some(value) = crate::user_upload::tcp::syscall(
        frame.registers[SYSCALL_NUMBER_REGISTER], frame.registers[0], frame.registers[1], frame.registers[2],
    ) {
        frame.registers[RESULT_REGISTER] = value;
        return;
    }
    let result = match frame.registers[SYSCALL_NUMBER_REGISTER] {
        #[cfg(all(feature = "pi-wifi", feature = "pi-managed"))]
        0x4701 if crate::user_upload::wifi::running() => {
            use crate::raspi2b::v3d;
            let [destination, background, foreground] = [frame.registers[0], frame.registers[1], frame.registers[2]];
            if crate::memory::bootstrap_user_range(destination, v3d::FRAME_BYTES as u32) {
                v3d::triangle(destination as *mut u8, background, foreground) as u32
            } else {
                SYSCALL_ERROR
            }
        }
        SYS_WRITE => write(frame.registers[0], frame.registers[1], frame.registers[2]),
        SYS_EXIT => exit(frame),
        SYS_REBOOT => crate::platform::reset(),
        _ => SYSCALL_ERROR,
    };
    frame.registers[RESULT_REGISTER] = result;
}

// Scheduled-process exit was handled above. A standalone Pi user image returns
// to the kernel trampoline through the SVC epilogue; QEMU exits via semihosting.
// The standalone path ignores the user's status argument.
unsafe fn exit(_frame: &mut UserFrame) -> u32 {
    #[cfg(any(
        feature = "pi-hardware",
        feature = "pi-lifecycle",
        feature = "pi-uart-user"
    ))]
    {
        #[cfg(feature = "pi-hardware")]
        crate::raspi2b::reset::stop_deadman();
        #[cfg(all(feature = "pi-wifi", feature = "pi-managed"))]
        crate::user_upload::wifi::exited(_frame.registers[0]);
        _frame.return_address =
            crate::arm_kernel_after_user_trampoline as *const () as usize as u32;
        // The epilogue restores this status, entering Supervisor mode with
        // IRQ/FIQ enabled before executing the replacement return address.
        const SUPERVISOR_RETURN_STATUS: u32 = 0x13;
        asm!("msr spsr_cxsf, {status}", status = in(reg) SUPERVISOR_RETURN_STATUS);
        0
    }
    #[cfg(not(any(
        feature = "pi-hardware",
        feature = "pi-lifecycle",
        feature = "pi-uart-user"
    )))]
    {
        exit_success()
    }
}

// Synchronous UART output. Arithmetic checks constrain the address interval;
// they do not inspect page-table permissions or catch a fault during the read.
unsafe fn write(fd: u32, address: u32, len: u32) -> u32 {
    if crate::scheduler::current_process().is_some()
        && !crate::memory::process_range_is_valid(address, len)
    {
        return SYSCALL_ERROR;
    }
    let end = address.saturating_add(len);
    if fd != STDOUT_FD
        || len > MAX_WRITE_BYTES
        || address < crate::memory::PROCESS_BASE
        || end > crate::memory::PROCESS_END
    {
        return SYSCALL_ERROR;
    }

    let mut console = Pl011::new(crate::platform::UART_BASE);
    for offset in 0..len {
        let byte = core::ptr::read_volatile((address + offset) as *const u8);
        console.write_byte(byte);
    }
    len
}

#[cfg(not(any(
    feature = "pi-hardware",
    feature = "pi-lifecycle",
    feature = "pi-uart-user"
)))]
unsafe fn exit_success() -> ! {
    // Arm semihosting: r0 selects the operation; r1 points to reason/status.
    const SYS_EXIT_EXTENDED: u32 = 0x20;
    const APPLICATION_EXIT_REASON: u32 = 0x0002_0026;
    const ARM_SEMIHOSTING_SVC: u32 = 0x123456;
    let block = [APPLICATION_EXIT_REASON, 0];
    asm!(
        "svc #{semihosting_svc}",
        in("r0") SYS_EXIT_EXTENDED,
        in("r1") block.as_ptr(),
        semihosting_svc = const ARM_SEMIHOSTING_SVC,
    );
    loop {
        asm!("wfi");
    }
}
