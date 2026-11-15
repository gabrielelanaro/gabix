use core::arch::asm;
#[cfg(feature = "virt")]
use gabix_kernel::memory::FrameAllocator;

const SECTION_COUNT: usize = 4096;
const SECTION_SIZE: u32 = 1024 * 1024;
const SECTION_DESCRIPTOR: u32 = 0b10;
const PRIVILEGED_READ_WRITE: u32 = 0b01 << 10;
const USER_READ_WRITE: u32 = 0b11 << 10;
const NORMAL_WRITE_BACK_SHARED: u32 = (1 << 16) | (1 << 3) | (1 << 2);
const NORMAL_NON_CACHEABLE: u32 = 0b001 << 12;
#[cfg(feature = "raspi2b")]
const USER_VIRTUAL_BASE: u32 = 0x4100_0000;
#[cfg(feature = "raspi2b")]
const USER_PHYSICAL_BASE: u32 = 0x0100_0000;

#[repr(C, align(16384))]
struct TranslationTable([u32; SECTION_COUNT]);

static mut BOOT_TABLE: TranslationTable = TranslationTable([0; SECTION_COUNT]);
static mut MMU_PROBE: u32 = 0;
#[cfg(feature = "virt")]
static mut FRAMES: FrameAllocator = FrameAllocator::empty();

/// Enable Cortex-A7 coherent requests before using the MMU or exclusive
/// accesses. Returns whether ACTLR.SMP bit 6 remained set after the write.
pub unsafe fn enable_smp() -> bool {
    let mut auxiliary_control: u32;
    asm!(
        "mrc p15, 0, {value}, c1, c0, 1",
        value = out(reg) auxiliary_control,
        options(nostack)
    );
    auxiliary_control |= 1 << 6;
    asm!(
        "mcr p15, 0, {value}, c1, c0, 1",
        "isb",
        value = in(reg) auxiliary_control,
        options(nostack)
    );
    asm!(
        "mrc p15, 0, {value}, c1, c0, 1",
        value = out(reg) auxiliary_control,
        options(nostack)
    );
    auxiliary_control & (1 << 6) != 0
}

#[cfg(feature = "virt")]
pub unsafe fn frame_is_reused() -> bool {
    let frames = &mut *core::ptr::addr_of_mut!(FRAMES);
    frames.reset(0x4000_0000, 0x4800_0000);
    // Keep the two fixed process allocations out of the frame allocator.
    frames.add_usable_range(0x4010_0000, 0x4200_0000);
    frames.add_usable_range(0x4240_0000, 0x47f0_0000);

    let first = frames.allocate().expect("ARM virt has usable RAM");
    let second = frames.allocate().expect("ARM virt has a second frame");
    frames.deallocate(first);
    let reused = frames.allocate().expect("released frame is reusable");

    first == reused && first != second
}

/// Install a bootstrap address space in which every virtual section maps to
/// the physical section with the same address.
///
/// Normal kernel RAM uses the data cache. User aliases below are noncacheable;
/// new translation tables are cleaned before the page-table walker uses them.
pub unsafe fn enable_identity_mapping() -> bool {
    let table = &mut *core::ptr::addr_of_mut!(BOOT_TABLE);
    for (index, descriptor) in table.0.iter_mut().enumerate() {
        let address = index as u32 * SECTION_SIZE;
        *descriptor = address
            | section_memory_attributes(address)
            | PRIVILEGED_READ_WRITE
            | SECTION_DESCRIPTOR;
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
    system_control |= 1 | (1 << 2);
    asm!("mcr p15, 0, {value}, c1, c0, 0", value = in(reg) system_control);
    asm!("isb");

    core::ptr::write_volatile(core::ptr::addr_of_mut!(MMU_PROBE), 0x4741_4249);
    core::ptr::read_volatile(core::ptr::addr_of!(MMU_PROBE)) == 0x4741_4249
}

fn section_memory_attributes(address: u32) -> u32 {
    #[cfg(feature = "raspi2b")]
    if address < 0x3f00_0000 {
        return NORMAL_WRITE_BACK_SHARED;
    }

    #[cfg(feature = "virt")]
    if (0x4000_0000..0x4800_0000).contains(&address) {
        return NORMAL_WRITE_BACK_SHARED;
    }

    0
}

pub unsafe fn allow_user_section(address: u32) {
    let index = (address / SECTION_SIZE) as usize;
    let table = &mut *core::ptr::addr_of_mut!(BOOT_TABLE);
    #[cfg(feature = "virt")]
    let physical = table.0[index] & 0xfff0_0000;
    #[cfg(feature = "raspi2b")]
    let physical = USER_PHYSICAL_BASE + (address & 0xfff0_0000) - USER_VIRTUAL_BASE;
    let descriptor = &mut table.0[index];
    *descriptor = physical | NORMAL_NON_CACHEABLE | USER_READ_WRITE | SECTION_DESCRIPTOR;

    let descriptor_address = descriptor as *mut u32 as u32;
    let zero = 0u32;
    asm!(
        "mcr p15, 0, {address}, c7, c10, 1",
        address = in(reg) descriptor_address
    );
    asm!("dsb");
    asm!("mcr p15, 0, {value}, c8, c7, 0", value = in(reg) zero);
    asm!("dsb");
    asm!("isb");
}

pub unsafe fn allow_user_range(start: u32, len: u32) {
    let Some(end) = start.checked_add(len) else {
        return;
    };
    let mut section = start / SECTION_SIZE * SECTION_SIZE;
    while section < end {
        allow_user_section(section);
        section = section.saturating_add(SECTION_SIZE);
    }
}

// First process implementation: two fixed 2 MiB allocations. These physical
// ranges are reserved here, separate from the kernel heap, loader and stacks.
// Both processes see their allocation at the same virtual addresses.
pub const PROCESS_BASE: u32 = 0x4100_0000;
pub const PROCESS_END: u32 = 0x4120_0000;
pub const PROCESS_STACK_TOP: u32 = PROCESS_END - 16;
pub const PROCESS_COUNT: usize = 2;
#[cfg(feature = "raspi2b")]
const PROCESS_PHYSICAL_BASE: u32 = 0x0200_0000;
#[cfg(feature = "virt")]
const PROCESS_PHYSICAL_BASE: u32 = 0x4200_0000;
static mut PROCESS_TABLES: [TranslationTable; PROCESS_COUNT] =
    [const { TranslationTable([0; SECTION_COUNT]) }; PROCESS_COUNT];

pub fn process_physical_base(id: usize) -> u32 {
    assert!(id < PROCESS_COUNT);
    PROCESS_PHYSICAL_BASE + id as u32 * 2 * SECTION_SIZE
}

/// Build a private address space. Kernel/device mappings stay privileged.
/// Only the two process sections grant user access; its stack is execute-never.
/// Safety: the process is stopped, and IRQs are masked during table changes.
pub unsafe fn prepare_process_table(id: usize) {
    assert!(id < PROCESS_COUNT);
    let table = core::ptr::addr_of_mut!(PROCESS_TABLES)
        .cast::<TranslationTable>()
        .add(id);
    for index in 0..SECTION_COUNT {
        let address = index as u32 * SECTION_SIZE;
        (*table).0[index] = address
            | section_memory_attributes(address)
            | PRIVILEGED_READ_WRITE
            | SECTION_DESCRIPTOR;
    }
    for section in 0..2 {
        (*table).0[(PROCESS_BASE / SECTION_SIZE) as usize + section] = (process_physical_base(id)
            + section as u32 * SECTION_SIZE)
            | NORMAL_NON_CACHEABLE
            | USER_READ_WRITE
            | SECTION_DESCRIPTOR
            | if section == 1 { 1 << 4 } else { 0 }; // XN: user stack is data.
    }
    // The page-table walker must see descriptors written into the data cache.
    // 32-byte steps cover both Cortex-A7 and QEMU Cortex-A15 cache lines.
    for offset in (0..core::mem::size_of::<TranslationTable>()).step_by(32) {
        let address = table.cast::<u8>().add(offset) as u32;
        asm!("mcr p15, 0, {address}, c7, c10, 1", address = in(reg) address);
    }
    asm!("dsb sy");
}

/// Select a process's mappings, or the boot mappings for the dispatcher.
/// Single core, IRQs masked. All tables map kernel code/stacks identically.
/// We flush the TLB on every change instead of assigning process ASIDs yet.
pub unsafe fn activate_process(id: Option<usize>) {
    let table = match id {
        Some(id) => {
            assert!(id < PROCESS_COUNT);
            core::ptr::addr_of!(PROCESS_TABLES)
                .cast::<TranslationTable>()
                .add(id) as u32
        }
        None => core::ptr::addr_of!(BOOT_TABLE) as u32,
    };
    asm!(
        "dsb sy",
        "mcr p15, 0, {table}, c2, c0, 0",
        "isb",
        "mcr p15, 0, {zero}, c8, c7, 0",
        "mcr p15, 0, {zero}, c7, c5, 6", // invalidate branch predictions
        "dsb sy", "isb",
        table = in(reg) table, zero = in(reg) 0u32,
    );
}

pub fn process_range_is_valid(address: u32, length: u32) -> bool {
    address >= PROCESS_BASE
        && address
            .checked_add(length)
            .is_some_and(|end| end <= PROCESS_END)
}

/// User code was written through its noncacheable user mapping. Invalidate
/// instruction-cache entries before executing it (including reused VA entries).
pub unsafe fn synchronize_process_code() {
    asm!("dsb sy", "mcr p15, 0, {zero}, c7, c5, 0", "dsb sy", "isb", zero = in(reg) 0u32);
}
