//! 8259 PIC (Programmable Interrupt Controller) setup.
//!
//! The PIC routes hardware interrupts from devices to the CPU. By default
//! it maps IRQs 0-15 to CPU vectors 8-15, which conflicts with CPU
//! exceptions (which use vectors 0-31). We remap to vectors 32-47 so
//! hardware interrupts don't get confused with exceptions.
//!
//! After `init()`, all IRQs are masked (no interrupts delivered yet).
//! Later slices will unmask specific ones — the timer first.

use crate::irq_mutex::IrqMutex;
use pic8259::ChainedPics;

/// Offset for the master PIC. IRQs 0-7 → CPU vectors 0x20-0x27 (32-39).
pub const PIC_1_OFFSET: u8 = 0x20;

/// Offset for the slave PIC. IRQs 8-15 → CPU vectors 0x28-0x2F (40-47).
pub const PIC_2_OFFSET: u8 = 0x28;

/// The chained 8259 PICs (master + slave = 15 usable IRQs).
///
/// Locked because we mutate it from multiple places: init, mask/unmask,
/// and sending EOI from interrupt handlers.
pub static PICS: IrqMutex<ChainedPics> =
    IrqMutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Initialize the PICs with our remapped offsets and unmask the timer.
///
/// `initialize()` saves and restores whatever mask state the firmware left
/// (typically everything masked in QEMU). So we explicitly unmask only the
/// timer (master PIC IRQ 0) after initialize.
///
/// Must be called before any hardware interrupts can fire.
pub fn init() {
    unsafe {
        let mut pics = PICS.lock();
        pics.initialize();
        // Bit N of mask = 1 means "IRQ N masked". 0xFE = 0b11111110:
        //   bit 0 = 0 → master IRQ 0 (timer) UNMASKED
        //   bits 1-7 = 1 → all other master IRQs masked
        // Slave mask 0xFF: all slave IRQs masked (no devices yet).
        pics.write_masks(0xFE, 0xFF);
    }
}
