use core::arch::asm;

const SECTION_COUNT: usize = 4096;
const SECTION_SIZE: u32 = 1024 * 1024;
const SECTION_DESCRIPTOR: u32 = 0b10;
const PRIVILEGED_READ_WRITE: u32 = 0b01 << 10;

#[repr(C, align(16384))]
struct TranslationTable([u32; SECTION_COUNT]);

static mut BOOT_TABLE: TranslationTable = TranslationTable([0; SECTION_COUNT]);
static mut MMU_PROBE: u32 = 0;

/// Install a bootstrap address space in which every virtual section maps to
/// the physical section with the same address.
///
/// Caches stay disabled. Later address-space work can replace individual
/// section entries without changing the MMU enable sequence.
pub unsafe fn enable_identity_mapping() -> bool {
    let table = &mut *core::ptr::addr_of_mut!(BOOT_TABLE);
    for (index, descriptor) in table.0.iter_mut().enumerate() {
        *descriptor = index as u32 * SECTION_SIZE | PRIVILEGED_READ_WRITE | SECTION_DESCRIPTOR;
    }

    let table_address = core::ptr::addr_of!(BOOT_TABLE) as u32;
    debug_assert_eq!(table_address & 0x3fff, 0);

    let zero = 0u32;
    let domain_zero_client = 1u32;
    asm!("dsb");
    asm!("mcr p15, 0, {value}, c2, c0, 2", value = in(reg) zero);
    asm!("mcr p15, 0, {value}, c2, c0, 0", value = in(reg) table_address);
    asm!("mcr p15, 0, {value}, c3, c0, 0", value = in(reg) domain_zero_client);
    asm!("mcr p15, 0, {value}, c8, c7, 0", value = in(reg) zero);
    asm!("dsb");
    asm!("isb");

    let mut system_control: u32;
    asm!("mrc p15, 0, {value}, c1, c0, 0", value = out(reg) system_control);
    system_control |= 1;
    asm!("mcr p15, 0, {value}, c1, c0, 0", value = in(reg) system_control);
    asm!("isb");

    core::ptr::write_volatile(core::ptr::addr_of_mut!(MMU_PROBE), 0x4741_4249);
    core::ptr::read_volatile(core::ptr::addr_of!(MMU_PROBE)) == 0x4741_4249
}
