#![no_std]
#![no_main]

const AF_INET: u64 = 2;
const SOCK_STREAM: u64 = 1;
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_SOCKET: u64 = 41;
const SYS_CONNECT: u64 = 42;

#[repr(C)]
struct SockAddrIn {
    family: u16,
    port: u16,
    address: [u8; 4],
    zero: [u8; 8],
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let fd = syscall3(SYS_SOCKET, AF_INET, SOCK_STREAM, 0);
    if fd == u64::MAX {
        exit(10);
    }

    let peer = SockAddrIn {
        family: AF_INET as u16,
        port: 8080u16.to_be(),
        address: [10, 0, 2, 2],
        zero: [0; 8],
    };
    if syscall3(SYS_CONNECT, fd, &peer as *const _ as u64, 16) != 0 {
        exit(11);
    }

    let payload = b"retransmit-me";
    if syscall3(SYS_WRITE, fd, payload.as_ptr() as u64, payload.len() as u64)
        != payload.len() as u64
    {
        exit(12);
    }

    let mut reply = [0u8; 1];
    if syscall3(SYS_READ, fd, reply.as_mut_ptr() as u64, 1) != 1 || reply[0] != b'!' {
        exit(13);
    }

    log(b"[tcp-client] response arrived after retransmit\n");
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

fn log(bytes: &[u8]) {
    let _ = syscall3(SYS_WRITE, 1, bytes.as_ptr() as u64, bytes.len() as u64);
}

fn exit(code: u64) -> ! {
    unsafe { core::arch::asm!("syscall", in("rax") 60, in("rdi") code, options(noreturn)) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(99)
}
