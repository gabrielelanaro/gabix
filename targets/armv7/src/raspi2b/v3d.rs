//! Synchronous 64x64 VC4 triangle rendering for the Pi 2B.
use core::arch::{asm, global_asm};
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

global_asm!(include_str!("v3d_lists.S"));
extern "C" {
    static gpu_data: u8;
    static gpu_data_end: u8;
    static gpu_work: u8;
    static gpu_work_end: u8;
    static mut pixels: [u8; FRAME_BYTES];
    static bin_list: u8;
    static bin_end: u8;
    static render_list: u8;
    static render_end: u8;
    static mut clear_color: [u8; 8];
    static fragment_code: u8;
    static fragment_end: u8;
    static shader_storage: u8;
    static mut vertices: [[u32; 6]; 3];
    static mut fragment_color: u32;
}

const MAILBOX: usize = 0x3f00_b880;
const V3D: usize = 0x3fc0_0000;
const IDENT0: usize = 0;
const BINNING: usize = 0;
const RENDERING: usize = 1;
const EXPECTED_ID: u32 = 0x02443356;
pub const FRAME_BYTES: usize = 64 * 64 * 4;

#[repr(align(64))]
struct Message([u32; 16]);
static mut MESSAGE: Message = Message([0; 16]);

unsafe fn read(address: usize) -> u32 {
    read_volatile(address as *const u32)
}
unsafe fn write(address: usize, value: u32) {
    write_volatile(address as *mut u32, value);
}
fn bus(address: *const u8) -> u32 {
    address as u32 | 0xc0000000
}
unsafe fn wait(mut done: impl FnMut() -> bool) -> bool {
    let counter = 0x3f003004;
    let start = read(counter);
    while !done() {
        if read(counter).wrapping_sub(start) > 1_000_000 {
            return false;
        }
    }
    true
}

// All ranges are cache-line isolated. The CPU hands dirty data to the device
// before submission and discards cached GPU output before readback.
unsafe fn cache(start: usize, end: usize, invalidate_only: bool) {
    for address in ((start & !31)..end).step_by(32) {
        if invalidate_only {
            asm!("mcr p15, 0, {a}, c7, c6, 1", a=in(reg) address);
        } else {
            asm!("mcr p15, 0, {a}, c7, c14, 1", a=in(reg) address);
        }
    }
    asm!("dsb sy");
}

// Reset released, clock busy and both AXI bridges accepting transactions.
unsafe fn v3d_ready() -> bool {
    read(0x3f10010c) & 0x7f == 0x7f
        && read(0x3f101038) & 0x80 != 0
        && read(0x3f00a008) & 3 == 0
        && read(0x3f00a00c) & 3 == 0
}

unsafe fn property(words: &[u32], response_bytes: u32) -> Result<*mut u32, i32> {
    let message = addr_of_mut!(MESSAGE.0).cast::<u32>();
    core::ptr::copy_nonoverlapping(words.as_ptr(), message, words.len());
    let address = message as usize;
    cache(address, address + 64, false);
    if !wait(|| read(MAILBOX + 0x38) & (1 << 31) == 0) {
        return Err(-10);
    }
    let request = bus(message.cast()) | 8;
    write(MAILBOX + 0x20, request);
    if !wait(|| read(MAILBOX + 0x18) & (1 << 30) == 0 && read(MAILBOX) == request) {
        return Err(-11);
    }
    cache(address, address + 64, true);
    if read_volatile(message.add(1)) != 0x80000000
        || read_volatile(message.add(4)) != (0x80000000 | response_bytes)
    {
        return Err(-12);
    }
    Ok(message)
}

unsafe fn set_v3d_power(on: bool) -> Result<(), i32> {
    if on && v3d_ready() {
        return Ok(());
    }
    // Firmware domain identifiers are one-based. Linux DT index 10 is V3D.
    let reply = property(&[32, 0, 0x00038030, 8, 0, 11, on as u32, 0], 8)?;
    if read_volatile(reply.add(6)) != on as u32
        || !wait(|| {
            if on {
                v3d_ready()
            } else {
                read(0x3f10010c) & 0x40 == 0
            }
        })
    {
        return Err(-12);
    }
    Ok(())
}

// The ARM publishes the source through the direct alias. Write the executable
// copy through the same VideoCore cache alias the QPU uses for instructions.
// On this Pi, changing the source through the direct alias alone allowed the
// QPU to execute an earlier shader at the same address.
unsafe fn publish_shader() -> Result<(), i32> {
    const CHANNEL: usize = 5;
    const ENABLE: usize = 0x3f007ff0;
    const CS: usize = 0x3f007000 + CHANNEL * 0x100;
    const CHANNEL_BIT: u32 = 1 << CHANNEL;
    const ACTIVE: u32 = 1;
    const END: u32 = 2;
    const ERROR: u32 = 1 << 8;
    const RESET: u32 = 1 << 31;
    const WAIT_FOR_WRITES: u32 = 1 << 28;
    const COPY: u32 = (1 << 8) | (1 << 4) | (1 << 3); // SRC_INC, DEST_INC, WAIT_RESP
    let reply = property(&[28, 0, 0x00060001, 4, 0, 0, 0], 4)?;
    if read_volatile(reply.add(5)) & CHANNEL_BIT == 0 || read(CS) & ACTIVE != 0 {
        return Err(-30);
    }
    let enabled = read(ENABLE);
    write(ENABLE, enabled | CHANNEL_BIT);
    write(CS, RESET);
    if !wait(|| read(CS) & RESET == 0) {
        if enabled & CHANNEL_BIT == 0 {
            write(ENABLE, read(ENABLE) & !CHANNEL_BIT);
        }
        return Err(-31);
    }
    // Reuse the aligned mailbox buffer after firmware has returned ownership.
    let block = addr_of_mut!(MESSAGE.0).cast::<u32>();
    let start = addr_of!(fragment_code);
    let size = addr_of!(fragment_end) as usize - start as usize;
    let words = [
        COPY,
        bus(start),
        addr_of!(shader_storage) as u32 | 0x40000000,
        size as u32,
        0,
        0,
        0,
        0,
    ];
    core::ptr::copy_nonoverlapping(words.as_ptr(), block, words.len());
    cache(block as usize, block as usize + 64, false);
    write(CS + 4, bus(block.cast()));
    write(CS, WAIT_FOR_WRITES | ACTIVE);
    if !wait(|| read(CS) & ACTIVE == 0) {
        // The watchdog recovers ownership if the DMA controller is wedged.
        loop {
            core::hint::spin_loop();
        }
    }
    let status = read(CS);
    write(CS, END);
    if enabled & CHANNEL_BIT == 0 {
        write(ENABLE, read(ENABLE) & !CHANNEL_BIT);
    }
    if status & (ERROR | END) != END {
        return Err(-32);
    }
    Ok(())
}

// Start a control thread and wait for its flush or completed-frame counter.
unsafe fn submit(thread: usize, start: *const u8, end: *const u8) -> bool {
    let counter = V3D + 0x134 + thread * 4;
    let previous = read(counter);
    write(V3D + 0x110 + thread * 4, bus(start));
    write(V3D + 0x108 + thread * 4, bus(end));
    wait(|| read(counter) != previous)
}

// Stop DMA before allowing another render. If the power manager also
// stalls, the already-armed board watchdog resets the Pi.
unsafe fn stop_stalled_job(error: i32) -> i32 {
    if set_v3d_power(false).is_err() {
        loop {
            core::hint::spin_loop();
        }
    }
    error
}

pub type Triangle = [[i16; 2]; 3];

/// Called by the single foreground SVC after validating the destination.
pub unsafe fn triangle(output: *mut u8, background: u32, foreground: u32, points: Triangle) -> i32 {
    // XY in the NV vertex record uses signed 12.4 fixed-point coordinates.
    if points.iter().flatten().any(|c| !(-2048..=2047).contains(c)) {
        return -1;
    }
    if let Err(error) = set_v3d_power(true) {
        return error;
    }
    let id = read(V3D + IDENT0);
    if id != EXPECTED_ID {
        return -13;
    }
    for (index, [x, y]) in points.into_iter().enumerate() {
        let xy = u32::from((x * 16) as u16) | (u32::from((y * 16) as u16) << 16);
        addr_of_mut!(vertices[index][0]).write(xy);
    }
    let clear = addr_of_mut!(clear_color).cast::<u32>();
    clear.write_unaligned(background);
    clear.add(1).write_unaligned(background);
    addr_of_mut!(fragment_color).write(foreground);
    cache(
        addr_of!(gpu_data) as usize,
        addr_of!(gpu_data_end) as usize,
        false,
    );
    cache(
        addr_of!(gpu_work) as usize,
        addr_of!(gpu_work_end) as usize,
        false,
    );
    write(V3D + 0x504, 0); // Allocate VPM to graphics shaders
    if let Err(error) = publish_shader() {
        return error;
    }
    write(V3D + 0x20, 5); // Enable and clear GPU L2
    write(V3D + 0x24, 0xffffffff); // Invalidate instruction, uniform and texture slice caches
    if !submit(BINNING, addr_of!(bin_list), addr_of!(bin_end)) {
        return stop_stalled_job(-20);
    }
    if !submit(RENDERING, addr_of!(render_list), addr_of!(render_end)) {
        return stop_stalled_job(-21);
    }
    cache(
        addr_of!(pixels) as usize,
        addr_of!(pixels) as usize + FRAME_BYTES,
        true,
    );
    core::ptr::copy_nonoverlapping(addr_of!(pixels).cast::<u8>(), output, FRAME_BYTES);
    FRAME_BYTES as i32
}
