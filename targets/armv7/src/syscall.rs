use crate::console::Pl011;
use core::arch::asm;
use gabix_kernel::BootConsole;

const SYS_WRITE: u32 = 1;
const SYS_EXIT: u32 = 60;
const SYS_REBOOT: u32 = 169;

#[repr(C)]
pub struct UserFrame {
    registers: [u32; 13],
    return_address: u32,
}

pub unsafe fn dispatch(frame: &mut UserFrame) {
    if let Some(id) = crate::scheduler::current_process() {
        match frame.registers[7] {
            SYS_EXIT => crate::process::exit_current(frame.registers[0]),
            39 => { frame.registers[0] = id as u32 + 1; return; } // getpid
            SYS_REBOOT => { frame.registers[0] = u32::MAX; return; }
            _ => {}
        }
    }
    let result = match frame.registers[7] {
        SYS_WRITE => write(frame.registers[0], frame.registers[1], frame.registers[2]),
        SYS_EXIT => exit(frame),
        SYS_REBOOT => crate::platform::reset(),
        _ => u32::MAX,
    };
    frame.registers[0] = result;
}

unsafe fn exit(_frame: &mut UserFrame) -> u32 {
    #[cfg(any(
        feature = "pi-hardware",
        feature = "pi-lifecycle",
        feature = "pi-uart-user"
    ))]
    {
        #[cfg(feature = "pi-hardware")]
        crate::raspi2b::reset::stop_deadman();
        _frame.return_address =
            crate::arm_kernel_after_user_trampoline as *const () as usize as u32;
        asm!("msr spsr_cxsf, {status}", status = in(reg) 0x13u32);
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

unsafe fn write(fd: u32, address: u32, len: u32) -> u32 {
    if crate::scheduler::current_process().is_some()
        && !crate::memory::process_range_is_valid(address, len) { return u32::MAX; }
    let end = address.saturating_add(len);
    if fd != 1 || len > 4096 || address < 0x4100_0000 || end > 0x4120_0000 {
        return u32::MAX;
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
    let block = [0x0002_0026u32, 0];
    asm!(
        "svc #0x123456",
        in("r0") 0x20u32,
        in("r1") block.as_ptr(),
    );
    loop {
        asm!("wfi");
    }
}
