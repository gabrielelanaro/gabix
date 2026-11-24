//! 8259 PIC (Programmable Interrupt Controller) setup.
//!
//! The two PICs route legacy device IRQs to CPU interrupt vectors.
//! We assign IRQs 0–15 to vectors 32–47, above CPU exceptions 0–31.
//! Initialization enables only the timer IRQ; all other PIC inputs stay masked.

use crate::irq_mutex::IrqMutex;
use pic8259::ChainedPics;

/// Offset for the master PIC. IRQs 0-7 → CPU vectors 0x20-0x27 (32-39).
pub const PIC_1_OFFSET: u8 = 0x20;

/// Offset for the slave PIC. IRQs 8-15 → CPU vectors 0x28-0x2F (40-47).
pub const PIC_2_OFFSET: u8 = 0x28;

const ALL_IRQS_MASKED: u8 = 0xff;
const TIMER_IRQ_BIT: u8 = 1 << 0;

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
        pics.write_masks(ALL_IRQS_MASKED & !TIMER_IRQ_BIT, ALL_IRQS_MASKED);
    }
}
