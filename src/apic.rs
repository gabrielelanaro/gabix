//! Local APIC setup and end-of-interrupt support.

use core::ptr::write_volatile;
use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::registers::model_specific::Msr;

const IA32_APIC_BASE: u32 = 0x1b;
const APIC_ENABLE: u64 = 1 << 11;
const APIC_PHYS_BASE: u64 = 0xfee0_0000;
const EOI_OFFSET: u64 = 0xb0;
const SPURIOUS_OFFSET: u64 = 0xf0;

static APIC_VIRT_BASE: AtomicU64 = AtomicU64::new(0);

/// Enable xAPIC mode and map its register page.
pub fn init() {
    let mut base_msr = Msr::new(IA32_APIC_BASE);
    let value = unsafe { base_msr.read() } | APIC_ENABLE;
    unsafe { base_msr.write(value) };

    let phys = value & 0x000f_ffff_ffff_f000;
    let virt = crate::memory::map_mmio(phys, 4096);
    APIC_VIRT_BASE.store(virt, Ordering::Release);

    // Software-enable the APIC and reserve 0xff as its spurious vector.
    unsafe { write_volatile((virt + SPURIOUS_OFFSET) as *mut u32, 0x1ff) };
    crate::println!("[apic] enabled at phys 0x{:x}, spurious vector=0xff", phys);
}

/// Acknowledge the current Local-APIC-delivered interrupt.
pub fn end_of_interrupt() {
    let base = APIC_VIRT_BASE.load(Ordering::Acquire);
    assert_ne!(base, 0, "Local APIC EOI before apic::init");
    unsafe { write_volatile((base + EOI_OFFSET) as *mut u32, 0) };
}

pub const MSI_ADDRESS: u32 = APIC_PHYS_BASE as u32;
