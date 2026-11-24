#![no_std]
#![no_main]

mod arm_tcp_common;
use arm_tcp_common::*;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    const SYS_V3D_TRIANGLE: u32 = 0x4701;
    let (listener, stream) = accept(18081);
    let mut request = [0u8; 8];
    let mut frame = [0u8; 64 * 64 * 4];
    loop {
        let mut used = 0;
        while used < request.len() {
            let remaining = &mut request[used..];
            let n = call(
                SYS_READ,
                stream,
                remaining.as_mut_ptr() as u32,
                remaining.len() as u32,
            );
            if used == 0 && n == 0 {
                finish(listener, stream);
            }
            if n == 0 || n == ERROR {
                exit(14);
            }
            used += n as usize;
        }
        let background = u32::from_le_bytes(request[..4].try_into().unwrap());
        let foreground = u32::from_le_bytes(request[4..].try_into().unwrap());
        if call(
            SYS_V3D_TRIANGLE,
            frame.as_mut_ptr() as u32,
            background,
            foreground,
        ) != frame.len() as u32
        {
            exit(17);
        }
        let mut sent = 0;
        while sent < frame.len() {
            let remaining = &frame[sent..];
            let n = call(
                SYS_WRITE,
                stream,
                remaining.as_ptr() as u32,
                remaining.len() as u32,
            );
            if n == 0 || n == ERROR {
                exit(15);
            }
            sent += n as usize;
        }
    }
}
