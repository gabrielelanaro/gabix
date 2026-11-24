#![no_std]
#![no_main]

const SYS_READ: u32 = 0;
const SYS_WRITE: u32 = 1;
const SYS_CLOSE: u32 = 3;
const SYS_SOCKET: u32 = 41;
const SYS_ACCEPT: u32 = 43;
const SYS_BIND: u32 = 49;
const SYS_LISTEN: u32 = 50;
const SYS_EXIT: u32 = 60;
const ERROR: u32 = u32::MAX;

fn call(number: u32, mut a: u32, b: u32, c: u32) -> u32 {
    unsafe {
        core::arch::asm!("svc #0", in("r7") number, inout("r0") a, in("r1") b, in("r2") c);
    }
    a
}

fn exit(code: u32) -> ! {
    unsafe {
        core::arch::asm!("svc #0", in("r7") SYS_EXIT, in("r0") code, options(noreturn));
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let listener = call(SYS_SOCKET, 2, 1, 0);
    let mut address = [0u8; 16];
    address[..2].copy_from_slice(&2u16.to_ne_bytes());
    address[2..4].copy_from_slice(&8080u16.to_be_bytes());
    if listener == ERROR || call(SYS_BIND, listener, address.as_ptr() as u32, 16) != 0 {
        exit(11);
    }
    if call(SYS_LISTEN, listener, 1, 0) != 0 {
        exit(12);
    }
    let stream = call(SYS_ACCEPT, listener, 0, 0);
    if stream == ERROR {
        exit(13);
    }

    let mut buf = [0u8; 256];
    loop {
        let n = call(SYS_READ, stream, buf.as_mut_ptr() as u32, buf.len() as u32);
        if n == ERROR {
            exit(14);
        }
        if n == 0 {
            break;
        }
        let mut sent = 0;
        while sent < n {
            let count = call(SYS_WRITE, stream, buf.as_ptr() as u32 + sent, n - sent);
            if count == 0 || count == ERROR {
                exit(15);
            }
            sent += count;
        }
    }
    if call(SYS_CLOSE, stream, 0, 0) != 0 || call(SYS_CLOSE, listener, 0, 0) != 0 {
        exit(16);
    }
    exit(0)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(99)
}
