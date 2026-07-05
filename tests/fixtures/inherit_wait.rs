// inherit_wait.elf checks three Lesson 18 behaviors:
// - forked children inherit open file descriptors
// - parent and child share the same File cursor for an inherited descriptor
// - wait4 can wait for one specific child PID and copy its exit status
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let fd = sys_open(b"/hello.txt\0");
    if fd == u64::MAX {
        fail(10, b"[inherit] open failed\n");
    }

    let mut parent_first = [0u8; 5];
    expect_read(fd, &mut parent_first, b"hello", 11, b"[inherit] parent first read failed\n");

    let child_pid = sys_fork();
    if child_pid == 0 {
        let mut child_buf = [0u8; 5];
        expect_read(fd, &mut child_buf, b" from", 21, b"[inherit] child read mismatch\n");
        sys_write(1, b"[inherit] child read shared fd\n");
        sys_exit(7);
    }

    let mut status = 0i64;
    let waited_pid = sys_wait_pid(child_pid, &mut status as *mut i64);
    if waited_pid != child_pid {
        fail(30, b"[inherit] wait returned wrong pid\n");
    }
    if status != 7 {
        fail(31, b"[inherit] wait status mismatch\n");
    }
    sys_write(1, b"[inherit] wait specific status=7\n");

    let mut parent_second = [0u8; 5];
    expect_read(fd, &mut parent_second, b" disk", 40, b"[inherit] parent cursor mismatch\n");
    sys_write(1, b"[inherit] parent saw child advance cursor\n");
    sys_exit(0);
}

fn expect_read(fd: u64, buf: &mut [u8; 5], expected: &[u8; 5], code: u64, msg: &[u8]) {
    let n = sys_read(fd, buf);
    if n != 5 || &buf[..] != &expected[..] {
        fail(code, msg);
    }
}

fn fail(code: u64, msg: &[u8]) -> ! {
    sys_write(1, msg);
    sys_exit(code);
}

fn sys_open(path: &[u8]) -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 2,
            in("rdi") path.as_ptr(),
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

fn sys_read(fd: u64, buf: &mut [u8]) -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 0,
            in("rdi") fd,
            in("rsi") buf.as_mut_ptr(),
            in("rdx") buf.len(),
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

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

fn sys_wait_pid(pid: u64, status_ptr: *mut i64) -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 61,
            in("rdi") pid,
            in("rsi") status_ptr,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

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
fn panic(_: &core::panic::PanicInfo) -> ! {
    sys_exit(99);
}
