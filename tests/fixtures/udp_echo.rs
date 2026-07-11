// udp_echo.elf asks the kernel to receive one UDP datagram on port 5555.
// The kernel owns UDP parsing, port dispatch, and the echo transmit path.
#![no_std]
#![no_main]

const SYS_NET_UDP_WAIT: u64 = 903;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    sys_write(1, b"[udp_echo] request UDP echo wait\n");
    let payload_len = sys_net_udp_wait();
    if payload_len == 0 || payload_len == u64::MAX {
        sys_write(1, b"[udp_echo] UDP echo failed\n");
        sys_exit(10);
    }

    sys_write(1, b"[udp_echo] guest UDP echo verified\n");
    sys_exit(0);
}

fn sys_net_udp_wait() -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_NET_UDP_WAIT,
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
