// fork_smoke.elf — exercises sys_fork, sys_write, sys_wait4, sys_exit.
//
// Parent calls sys_fork. Child sees pid=0 and prints "[fork_smoke] child",
// then exits. Parent prints "[fork_smoke] parent", waits for child via
// sys_wait4, then exits. The wrapper script (tests/test_fork_smoke.sh)
// greps serial output for both markers — order doesn't matter, but both
// must appear.
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let pid = sys_fork();
    if pid == 0 {
        sys_write(1, b"[fork_smoke] child\n");
        sys_exit(0);
    } else {
        sys_write(1, b"[fork_smoke] parent\n");
        sys_wait_any();
        sys_exit(0);
    }
}

/// sys_fork() — Linux x86-64 ABI: rax = 57. No args.
/// Returns child PID in parent, 0 in child. Clobbers rcx and r11.
fn sys_fork() -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 57,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

/// sys_write(fd, buf) — Linux x86-64 ABI: rax = 1, rdi = fd, rsi = buf, rdx = len.
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

/// sys_wait_any — wait for any child to exit. Our kernel's wait4 subset uses
/// arg1 as the target pid (-1 = any child) and arg2 as the optional status
/// pointer.
fn sys_wait_any() -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 61,
            in("rdi") u64::MAX, // target pid = -1, any child
            in("rsi") 0u64,     // exit_status_ptr = NULL
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

/// sys_exit(code) — Linux x86-64 ABI: rax = 60, rdi = exit code.
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
