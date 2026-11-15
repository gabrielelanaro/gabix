//! Local APIC setup and end-of-interrupt support.

use core::ptr::write_volatile;
use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::registers::model_specific::Msr;

const IA32_APIC_BASE: u32 = 0x1b;
const APIC_GLOBAL_ENABLE: u64 = 1 << 11;
const APIC_BASE_ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
const APIC_REGISTER_PAGE_BYTES: u64 = 4096;
const APIC_SOFTWARE_ENABLE: u32 = 1 << 8;
const SPURIOUS_VECTOR: u32 = 0xff;
const EOI_ACKNOWLEDGE: u32 = 0;
// MSI messages use this address encoding, independently of our MMIO mapping.
const MSI_ADDRESS_PREFIX: u32 = 0xfee0_0000;
const EOI_OFFSET: u64 = 0xb0;
const SPURIOUS_OFFSET: u64 = 0xf0;

static APIC_VIRT_BASE: AtomicU64 = AtomicU64::new(0);

/// Enable and map the local APIC, assuming the boot environment uses xAPIC.
/// Existing mode bits are preserved; xAPIC must already be selected.
pub fn init() {
    let mut base_msr = Msr::new(IA32_APIC_BASE);
    let value = unsafe { base_msr.read() } | APIC_GLOBAL_ENABLE;
    unsafe { base_msr.write(value) };

    let phys = value & APIC_BASE_ADDRESS_MASK;
    let virt = crate::memory::map_mmio(phys, APIC_REGISTER_PAGE_BYTES);
    APIC_VIRT_BASE.store(virt, Ordering::Release);

    // Software-enable the APIC and reserve 0xff as its spurious vector.
    unsafe {
        write_volatile(
            (virt + SPURIOUS_OFFSET) as *mut u32,
            APIC_SOFTWARE_ENABLE | SPURIOUS_VECTOR,
        )
    };
    crate::println!("[apic] enabled at phys 0x{:x}, spurious vector=0xff", phys);
}

/// Acknowledge the current Local-APIC-delivered interrupt.
pub fn end_of_interrupt() {
    let base = APIC_VIRT_BASE.load(Ordering::Acquire);
    assert_ne!(base, 0, "Local APIC EOI before apic::init");
    unsafe { write_volatile((base + EOI_OFFSET) as *mut u32, EOI_ACKNOWLEDGE) };
}

/// Physical-destination MSI address for APIC ID 0 (our single-CPU setup).
/// Destination and routing bits are zero; each device supplies its vector
/// separately in the MSI message data field.
pub const MSI_ADDRESS: u32 = MSI_ADDRESS_PREFIX;
