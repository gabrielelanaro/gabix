// icmp_ping.elf asks the kernel to perform one ICMP echo round trip.
//
// The user program owns the intent: “ping the gateway.” The kernel owns the
// network device and the packet path behind the syscall boundary.
#![no_std]
#![no_main]

const SYS_NET_ICMP_PING: u64 = 902;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    sys_write(1, b"[icmp_ping] request gateway echo\n");
    if sys_net_icmp_ping() != 0 {
        sys_write(1, b"[icmp_ping] gateway echo failed\n");
        sys_exit(10);
    }

    sys_write(1, b"[icmp_ping] gateway echo verified\n");
    sys_exit(0);
}

fn sys_net_icmp_ping() -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_NET_ICMP_PING,
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
