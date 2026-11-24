//! Uploaded-program ABI for one synchronous draw into a 64 × 64 BGRA frame.
pub type Triangle = [[i16; 2]; 3];
pub type Frame = [u8; 64 * 64 * 4];

pub fn draw(
    frame: &mut Frame,
    points: &Triangle,
    background: u32,
    foreground: u32,
) -> Result<(), ()> {
    let mut result = frame.as_mut_ptr() as u32;
    unsafe {
        core::arch::asm!("svc #0", in("r7") 0x4703u32, inout("r0") result,
            in("r1") background, in("r2") foreground, in("r3") points.as_ptr());
    }
    if result == frame.len() as u32 {
        Ok(())
    } else {
        Err(())
    }
}
