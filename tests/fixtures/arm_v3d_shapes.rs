#![no_std]
#![no_main]

mod arm_v3d;
use arm_v3d::{draw, Frame};

const BG: u32 = 0xff19_4296;
const FG: u32 = 0xffff_cd28;

fn exit(status: u32) -> ! {
    unsafe {
        core::arch::asm!("svc #0", in("r7") 60u32, in("r0") status, options(noreturn));
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut frame: Frame = [0; 64 * 64 * 4];
    // These edges miss every pixel center, so the oracle is independent of
    // the GPU's tie-breaking rule. Both shapes use exactly the same colors.
    let shapes = [[[32, 8], [8, 56], [56, 56]], [[4, 12], [44, 12], [4, 32]]];
    for (index, shape) in shapes.iter().enumerate() {
        if draw(&mut frame, shape, BG, FG).is_err() {
            exit(10 + index as u32);
        }
        for (i, pixel) in frame.chunks_exact(4).enumerate() {
            let (x, y) = ((i % 64) as i32, (i / 64) as i32);
            let inside = if index == 0 {
                (8..56).contains(&y) && 2 * (2 * x + 1 - 64).abs() < 2 * y + 1 - 16
            } else {
                x >= 4 && y >= 12 && (2 * x + 1 - 8) + 2 * (2 * y + 1 - 24) < 80
            };
            if pixel != (if inside { FG } else { BG }).to_le_bytes() {
                exit(20 + index as u32);
            }
        }
    }
    if draw(&mut frame, &[[2048, 0], [0, 0], [1, 1]], BG, FG).is_ok() {
        exit(30);
    }
    exit(0)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(99)
}
