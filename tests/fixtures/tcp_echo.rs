#![no_std]
#![no_main]

const AF_INET: u64 = 2;
const SOCK_STREAM: u64 = 1;
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_CLOSE: u64 = 3;
const SYS_SOCKET: u64 = 41;
const SYS_BIND: u64 = 49;
const SYS_LISTEN: u64 = 50;
const SYS_ACCEPT: u64 = 43;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let listener = syscall3(SYS_SOCKET, AF_INET, SOCK_STREAM, 0);
    if listener == u64::MAX {
        exit(10);
    }
    let address = sockaddr(8080);
    if syscall3(SYS_BIND, listener, address.as_ptr() as u64, 16) != 0 {
        exit(11);
    }
    if syscall2(SYS_LISTEN, listener, 4) != 0 {
        exit(12);
    }
    // The current descriptor table allocates from zero while stdout and
    // stderr remain syscall-layer conventions. Reserve descriptors 1 and 2
    // so accept returns a stream descriptor that does not collide with them.
    if syscall3(SYS_SOCKET, AF_INET, SOCK_STREAM, 0) == u64::MAX {
        exit(18);
    }
    if syscall3(SYS_SOCKET, AF_INET, SOCK_STREAM, 0) == u64::MAX {
        exit(19);
    }
    log(b"[tcp-echo] listening on 10.0.2.15:8080\n");

    let connection = syscall1(SYS_ACCEPT, listener);
    if connection == u64::MAX {
        exit(13);
    }
    log(b"[tcp-echo] accepted connection\n");

    let mut buffer = [0u8; 256];
    loop {
        let count = syscall3(
            SYS_READ,
            connection,
            buffer.as_mut_ptr() as u64,
            buffer.len() as u64,
        );
        if count == u64::MAX {
            exit(14);
        }
        if count == 0 {
            break;
        }
        let _ = syscall3(SYS_WRITE, connection, buffer.as_ptr() as u64, count);
        log(b"[tcp-echo] echoed payload\n");
    }
    let _ = syscall1(SYS_CLOSE, connection);
    let _ = syscall1(SYS_CLOSE, listener);
    log(b"[tcp-echo] peer FIN acknowledged; server FIN sent\n");
    exit(0)
}

fn sockaddr(port: u16) -> [u8; 16] {
    let mut value = [0u8; 16];
    value[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    value[2..4].copy_from_slice(&port.to_be_bytes());
    value
}

fn syscall1(n: u64, a: u64) -> u64 {
    let result;
    unsafe {
        core::arch::asm!("syscall", in("rax") n, inlateout("rdi") a => _,
            lateout("rax") result, lateout("rcx") _, lateout("r11") _)
    }
    result
}

fn syscall2(n: u64, a: u64, b: u64) -> u64 {
    let result;
    unsafe {
        core::arch::asm!("syscall", in("rax") n, inlateout("rdi") a => _,
            inlateout("rsi") b => _,
            lateout("rax") result, lateout("rcx") _, lateout("r11") _)
    }
    result
}

fn syscall3(n: u64, a: u64, b: u64, c: u64) -> u64 {
    let result;
    unsafe {
        core::arch::asm!("syscall", in("rax") n, inlateout("rdi") a => _,
            inlateout("rsi") b => _, inlateout("rdx") c => _,
            lateout("rax") result, lateout("rcx") _, lateout("r11") _)
    }
    result
}

fn log(message: &[u8]) {
    let _ = syscall3(SYS_WRITE, 1, message.as_ptr() as u64, message.len() as u64);
}

fn exit(code: u64) -> ! {
    unsafe { core::arch::asm!("syscall", in("rax") 60u64, in("rdi") code, options(noreturn)) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(99)
}
