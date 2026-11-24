#![no_std]
#![no_main]

mod arm_tcp_common;
mod arm_v3d;
use arm_tcp_common::*;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let points = [[32, 8], [8, 56], [56, 56]];
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
        if arm_v3d::draw(&mut frame, &points, background, foreground).is_err() {
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
