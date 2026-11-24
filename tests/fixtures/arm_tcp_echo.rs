#![no_std]
#![no_main]

mod arm_tcp_common;
use arm_tcp_common::*;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let (listener, stream) = accept(8080);
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
    finish(listener, stream)
}
