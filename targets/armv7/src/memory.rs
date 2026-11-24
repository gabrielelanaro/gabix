//! Fixed ARM section tables for bootstrap and two process allocations.
//! Tables are static and unlocked: single-core callers must exclude preemption
//! and other access while mutating them. There is no table/frame reclamation.
use core::arch::asm;
#[cfg(feature = "virt")]
use gabix_kernel::memory::FrameAllocator;

// One first-level entry maps 1 MiB; 4096 entries cover the 32-bit address space.
const SECTION_COUNT: usize = 4096;
const TRANSLATION_TABLE_ALIGNMENT: u32 = 16 * 1024;
const SECTION_ADDRESS_MASK: u32 = 0xfff0_0000;
const SECTION_EXECUTE_NEVER: u32 = 1 << 4;
const ACTLR_SMP: u32 = 1 << 6;
const SCTLR_MMU_ENABLE: u32 = 1;
const SCTLR_DATA_CACHE_ENABLE: u32 = 1 << 2;
const DOMAIN_ZERO_CLIENT: u32 = 1;
const MMU_PROBE_PATTERN: u32 = 0x4741_4249;
// Existing maintenance stride; repeated operations on the same line are safe.
const TABLE_CACHE_CLEAN_STRIDE: usize = 32;
const SECTION_SIZE: u32 = 1024 * 1024;
const SECTION_DESCRIPTOR: u32 = 0b10;
const SECTION_ACCESS_SHIFT: u32 = 10;
const SECTION_TEX_SHIFT: u32 = 12;
const SECTION_SHAREABLE: u32 = 1 << 16;
const SECTION_CACHEABLE: u32 = 1 << 3;
const SECTION_BUFFERABLE: u32 = 1 << 2;
const PRIVILEGED_READ_WRITE: u32 = 0b01 << SECTION_ACCESS_SHIFT;
const USER_READ_WRITE: u32 = 0b11 << SECTION_ACCESS_SHIFT;
// Short-descriptor attributes assume SCTLR.TRE=0 (no TEX remapping).
const NORMAL_WRITE_BACK_SHARED: u32 =
    SECTION_SHAREABLE | SECTION_CACHEABLE | SECTION_BUFFERABLE;
const NORMAL_NON_CACHEABLE: u32 = 0b001 << SECTION_TEX_SHIFT;
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
    auxiliary_control |= ACTLR_SMP;
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
    auxiliary_control & ACTLR_SMP != 0
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
/// All 4096 sections get descriptors, including addresses outside real RAM.
/// Normal kernel RAM uses the data cache; later user aliases are noncacheable.
/// Safety: boot-time exclusive access with a compatible initial CPU/cache state.
/// This initial table is not cleaned from data cache before installation; callers
/// must ensure its stores are visible to the walker. Only SCTLR M/C are set here:
/// other control bits are inherited, including TEX remapping and access flags.
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
    debug_assert_eq!(table_address & (TRANSLATION_TABLE_ALIGNMENT - 1), 0);

    let zero = 0u32;
    asm!("dsb");
    // TTBCR=0: use TTBR0 for the whole virtual address space.
    asm!("mcr p15, 0, {value}, c2, c0, 2", value = in(reg) zero);
    // TTBR0 points the hardware table walker at our section descriptors.
    asm!("mcr p15, 0, {value}, c2, c0, 0", value = in(reg) table_address);
    // DACR: domain 0 uses the access permissions stored in each descriptor.
    asm!("mcr p15, 0, {value}, c3, c0, 0", value = in(reg) DOMAIN_ZERO_CLIENT);
    asm!("mcr p15, 0, {value}, c8, c7, 0", value = in(reg) zero);
    asm!("dsb");
    asm!("isb");

    let mut system_control: u32;
    asm!("mrc p15, 0, {value}, c1, c0, 0", value = out(reg) system_control);
    system_control |= SCTLR_MMU_ENABLE | SCTLR_DATA_CACHE_ENABLE;
    asm!("mcr p15, 0, {value}, c1, c0, 0", value = in(reg) system_control);
    asm!("isb");

    core::ptr::write_volatile(core::ptr::addr_of_mut!(MMU_PROBE), MMU_PROBE_PATTERN);
    core::ptr::read_volatile(core::ptr::addr_of!(MMU_PROBE)) == MMU_PROBE_PATTERN
}

// Hard-coded platform ranges, independent of detected physical RAM capacity.
// With TEX remapping disabled, the zero encoding outside RAM is Strongly-ordered.
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

/// Replace the containing boot-table section with a user-writable mapping.
/// Safety: serialize access to BOOT_TABLE and keep executing kernel code mapped.
/// On Pi, address must belong to the intended fixed user alias range; this
/// helper computes its physical address without validating that range.
/// Cache maintenance below cleans the descriptor only. Changing the mapped
/// memory's cacheability requires the caller to avoid dirty/conflicting aliases.
pub unsafe fn allow_user_section(address: u32) {
    let index = (address / SECTION_SIZE) as usize;
    let table = &mut *core::ptr::addr_of_mut!(BOOT_TABLE);
    #[cfg(feature = "virt")]
    let physical = table.0[index] & SECTION_ADDRESS_MASK;
    #[cfg(feature = "raspi2b")]
    let physical = USER_PHYSICAL_BASE + (address & SECTION_ADDRESS_MASK) - USER_VIRTUAL_BASE;
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

/// Grant access to every section touched by this byte range. Overflow returns
/// without changes. An unaligned zero-length range still selects its section.
/// Safety: same exclusive table access and valid alias requirements as allow_user_section().
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
const PROCESS_STACK_TOP_PADDING: u32 = 16;
pub const PROCESS_STACK_TOP: u32 = PROCESS_END - PROCESS_STACK_TOP_PADDING;
const PROCESS_SECTION_COUNT: usize = 2;
const PROCESS_STACK_SECTION: usize = 1;
pub const PROCESS_COUNT: usize = 2;
#[cfg(feature = "raspi2b")]
const PROCESS_PHYSICAL_BASE: u32 = 0x0200_0000;
#[cfg(feature = "virt")]
const PROCESS_PHYSICAL_BASE: u32 = 0x4200_0000;
static mut PROCESS_TABLES: [TranslationTable; PROCESS_COUNT] =
    [const { TranslationTable([0; SECTION_COUNT]) }; PROCESS_COUNT];

pub fn process_physical_base(id: usize) -> u32 {
    assert!(id < PROCESS_COUNT);
    PROCESS_PHYSICAL_BASE + id as u32 * PROCESS_SECTION_COUNT as u32 * SECTION_SIZE
}

/// Build a private address space. Kernel/device mappings stay privileged.
/// Only the two process sections grant user access; its stack is execute-never.
/// Safety: the process is stopped, and IRQs are masked during table changes.
/// Privileged identity aliases to process RAM remain cacheable. Callers must
/// avoid accessing that RAM through conflicting cached/noncacheable aliases;
/// constructing these tables does not clean the process payload's cache lines.
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
    for section in 0..PROCESS_SECTION_COUNT {
        (*table).0[(PROCESS_BASE / SECTION_SIZE) as usize + section] = (process_physical_base(id)
            + section as u32 * SECTION_SIZE)
            | NORMAL_NON_CACHEABLE
            | USER_READ_WRITE
            | SECTION_DESCRIPTOR
            | if section == PROCESS_STACK_SECTION {
                SECTION_EXECUTE_NEVER
            } else {
                0
            };
    }
    // The page-table walker must see descriptors written into the data cache.
    // 32-byte steps cover both Cortex-A7 and QEMU Cortex-A15 cache lines.
    for offset in (0..core::mem::size_of::<TranslationTable>()).step_by(TABLE_CACHE_CLEAN_STRIDE) {
        let address = table.cast::<u8>().add(offset) as u32;
        asm!("mcr p15, 0, {address}, c7, c10, 1", address = in(reg) address);
    }
    asm!("dsb sy");
}

/// Select a process's mappings, or the boot mappings for the dispatcher.
/// Single core, IRQs masked. All tables map kernel code/stacks identically.
/// Every change invalidates local translations; process ASIDs are not assigned.
/// The selected table must already be initialized and cleaned. This performs no
/// remote-core invalidation and does not normalize other CPU control settings.
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

/// Check only the fixed virtual interval and addition overflow. This does not
/// inspect mappings or permissions. A zero-length range at PROCESS_END is valid.
pub fn process_range_is_valid(address: u32, length: u32) -> bool {
    address >= PROCESS_BASE
        && address
            .checked_add(length)
            .is_some_and(|end| end <= PROCESS_END)
}

/// User code was written through its noncacheable user mapping. Invalidate
/// instruction-cache entries before executing it (including reused VA entries).
/// Safety: caller must finish writing code and exclude concurrent execution.
/// This does not clean a dirty data-cache alias of the code.
pub unsafe fn synchronize_process_code() {
    asm!("dsb sy", "mcr p15, 0, {zero}, c7, c5, 0", "dsb sy", "isb", zero = in(reg) 0u32);
}


// The Wi-Fi ELF uses BOOT_TABLE while in user mode. Check each touched
// section before dereferencing a user buffer, including a buffer on its stack.
#[cfg(all(feature = "pi-wifi", feature = "pi-managed"))]
pub fn bootstrap_user_range(address: u32, length: u32) -> bool {
    let Some(last) = length.checked_sub(1).and_then(|n| address.checked_add(n)) else {
        return false;
    };
    if address < PROCESS_BASE || last >= crate::user::USER_STACK_TOP { return false; }
    (address / SECTION_SIZE..=last / SECTION_SIZE).all(|section| unsafe {
        let descriptor = (*core::ptr::addr_of!(BOOT_TABLE)).0[section as usize];
        descriptor & USER_READ_WRITE == USER_READ_WRITE
    })
}
