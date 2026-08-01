#![no_std]
#![no_main]

const KERNEL_POINTER: u64 = 0xffff_ffff_8000_0000;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let result = syscall3(1, 1, KERNEL_POINTER, 4);
    if result != u64::MAX {
        exit(10);
    }

    write(b"[invalid-pointer] error returned\n");
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
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 60,
            in("rdi") code,
            options(noreturn),
        );
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(99)
}
