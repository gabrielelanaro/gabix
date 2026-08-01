#![no_std]
#![no_main]

const CLOCK_MONOTONIC: u64 = 1;
const SYS_CLOCK_GETTIME: u64 = 228;

#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq)]
struct Timespec {
    seconds: i64,
    nanoseconds: i64,
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let started = now();
    let mut current = started;
    while current == started {
        current = now();
    }
    write(b"[timer] monotonic time advanced\n");
    exit(0)
}

fn now() -> Timespec {
    let mut value = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    if syscall2(
        SYS_CLOCK_GETTIME,
        CLOCK_MONOTONIC,
        &mut value as *mut _ as u64,
    ) != 0
    {
        exit(10);
    }
    value
}

fn syscall2(number: u64, arg1: u64, arg2: u64) -> u64 {
    let result;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") number,
            in("rdi") arg1,
            in("rsi") arg2,
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

fn exit(code: u64) -> ! {
    unsafe { core::arch::asm!("syscall", in("rax") 60, in("rdi") code, options(noreturn)) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(99)
}
