#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! {
    write(b"[exec-test] before exec\n");
    let result = syscall1(59, b"/hello.elf\0".as_ptr() as u64);
    let _ = result;
    write(b"[exec-test] exec returned\n");
    exit(10)
}

fn syscall1(number: u64, arg1: u64) -> u64 {
    let result;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") number,
            in("rdi") arg1,
            lateout("rax") result,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    result
}

fn write(bytes: &[u8]) {
    let result: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1,
            in("rdi") 1,
            in("rsi") bytes.as_ptr(),
            in("rdx") bytes.len(),
            lateout("rax") result,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    let _ = result;
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
