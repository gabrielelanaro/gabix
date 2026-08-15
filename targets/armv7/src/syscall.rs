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
    let result = match frame.registers[7] {
        SYS_WRITE => write(frame.registers[0], frame.registers[1], frame.registers[2]),
        SYS_EXIT => exit(frame),
        SYS_REBOOT => crate::platform::reset(),
        _ => u32::MAX,
    };
    frame.registers[0] = result;
}

unsafe fn exit(_frame: &mut UserFrame) -> u32 {
    #[cfg(any(feature = "pi-hardware", feature = "pi-lifecycle"))]
    {
        _frame.return_address = crate::arm_kernel_after_user as *const () as usize as u32;
        asm!("msr spsr_cxsf, {status}", status = in(reg) 0x13u32);
        0
    }
    #[cfg(not(any(feature = "pi-hardware", feature = "pi-lifecycle")))]
    {
        exit_success()
    }
}

unsafe fn write(fd: u32, address: u32, len: u32) -> u32 {
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

#[cfg(not(any(feature = "pi-hardware", feature = "pi-lifecycle")))]
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
