#![no_std]
#![no_main]

const EXPECTED: &[u8] = b"gabix-known-disk-data\n";

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let fd = syscall3(2, b"/known-block.bin\0".as_ptr() as u64, 0, 0);
    if fd == u64::MAX {
        exit(10);
    }

    let mut bytes = [0u8; EXPECTED.len()];
    let count = syscall3(0, fd, bytes.as_mut_ptr() as u64, bytes.len() as u64);
    if count != bytes.len() as u64 || bytes != EXPECTED {
        exit(11);
    }

    write(b"[disk] known data returned\n");
    exit(0)
}

fn syscall3(number: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    let result;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") number,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            lateout("rax") result,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    result
}

fn write(bytes: &[u8]) {
    let _ = syscall3(1, 1, bytes.as_ptr() as u64, bytes.len() as u64);
}

fn exit(code: u64) -> ! {
    unsafe { core::arch::asm!("syscall", in("rax") 60, in("rdi") code, options(noreturn)) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(99)
}
