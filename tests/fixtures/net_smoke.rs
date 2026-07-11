// net_smoke.elf asks the kernel to drive the virtio-net device.
//
// The program runs in ring 3. It triggers Lesson 23 TX and RX through
// explicit syscalls so the network proof starts from a user fixture.
#![no_std]
#![no_main]

const SYS_NET_TX_TEST: u64 = 900;
const SYS_NET_RX_WAIT: u64 = 901;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    sys_write(1, b"[net_smoke] request ipv4 tx\n");
    if sys_net_tx_test() != 0 {
        fail(10, b"[net_smoke] ipv4 tx failed\n");
    }

    sys_write(1, b"[net_smoke] request ipv4 rx\n");
    let rx_len = sys_net_rx_wait();
    if rx_len == 0 || rx_len == u64::MAX {
        fail(20, b"[net_smoke] ipv4 rx failed\n");
    }

    sys_write(1, b"[net_smoke] user-triggered network rx ok\n");
    sys_exit(0);
}

fn fail(code: u64, msg: &[u8]) -> ! {
    sys_write(1, msg);
    sys_exit(code);
}

fn sys_net_tx_test() -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_NET_TX_TEST,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
        );
    }
    ret
}

fn sys_net_rx_wait() -> u64 {
    let ret;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_NET_RX_WAIT,
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
