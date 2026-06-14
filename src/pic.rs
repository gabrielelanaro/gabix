//! 8259 PIC (Programmable Interrupt Controller) setup.
//!
//! The PIC routes hardware interrupts from devices to the CPU. By default
//! it maps IRQs 0-15 to CPU vectors 8-15, which conflicts with CPU
//! exceptions (which use vectors 0-31). We remap to vectors 32-47 so
//! hardware interrupts don't get confused with exceptions.
//!
//! After `init()`, all IRQs are masked (no interrupts delivered yet).
//! Later slices will unmask specific ones — the timer first.

use spin::Mutex;
use pic8259::ChainedPics;

/// Offset for the master PIC. IRQs 0-7 → CPU vectors 0x20-0x27 (32-39).
pub const PIC_1_OFFSET: u8 = 0x20;

/// Offset for the slave PIC. IRQs 8-15 → CPU vectors 0x28-0x2F (40-47).
pub const PIC_2_OFFSET: u8 = 0x28;

/// The chained 8259 PICs (master + slave = 15 usable IRQs).
///
/// Locked because we mutate it from multiple places: init, mask/unmask,
/// and sending EOI from interrupt handlers.
pub static PICS: Mutex<ChainedPics> = Mutex::new(unsafe {
    ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET)
});

/// Initialize the PICs with our remapped offsets.
///
/// Must be called before any hardware interrupts can fire.
/// After this, all IRQs are masked — we'll unmask them per-source.
pub fn init() {
    unsafe {
        PICS.lock().initialize();
    }
}
