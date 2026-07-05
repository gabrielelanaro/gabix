// hello.elf — the mission-landing user program (Lesson 16).
//
// Writes "hello, world\n" to stdout (fd=1) via sys_write, then exits.
// Demonstrates that exec() produces a ring-3 program that can call
// back into the kernel via the syscall instruction.
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let msg: &[u8] = b"hello, world\n";
    sys_write(1, msg);
    sys_exit(0);
}

/// sys_write(fd, buf, len) — Linux x86-64 ABI:
///   rax = 1, rdi = fd, rsi = buf ptr, rdx = len
/// Returns byte count in rax. Clobbers rcx and r11.
fn sys_write(fd: u64, buf: &[u8]) -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 1,
            in("rdi") fd,
            in("rsi") buf.as_ptr(),
            in("rdx") buf.len(),
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

/// sys_exit(code) — Linux x86-64 ABI: rax = 60, rdi = exit code.
/// Never returns. The kernel handler terminates the user program.
fn sys_exit(code: u64) -> ! {
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
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
