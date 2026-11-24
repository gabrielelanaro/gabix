use core::arch::naked_asm;
use gabix_kernel::elf::Elf32;

#[cfg(feature = "virt")]
const USER_HEADER: *const u32 = 0x4100_0000 as *const u32;
#[cfg(feature = "virt")]
const USER_MAGIC: u32 = 0x5849_4247; // Little-endian bytes "GBIX".
#[cfg(feature = "virt")]
const USER_HEADER_ENTRY_WORD: usize = 1;
const CPSR_SYSTEM_MODE: u32 = 0x1f;
const CPSR_SUPERVISOR_MODE: u32 = 0x13;
const INITIAL_USER_CPSR: u32 = 0x10; // User mode, ARM instructions, IRQ/FIQ enabled.
const USER_IMAGE_START: u32 = 0x4100_0000;
const STACK_RESERVED_BYTES: u32 = 0x10_0000;
pub const USER_STACK_TOP: u32 = 0x47df_f000;

#[cfg(feature = "virt")]
/// Read the flat-image header placed in RAM by the QEMU test loader.
/// The magic is the only validation here; the entry address is returned as-is.
pub fn loaded_entry() -> Option<u32> {
    unsafe {
        if core::ptr::read_volatile(USER_HEADER) == USER_MAGIC {
            Some(core::ptr::read_volatile(USER_HEADER.add(USER_HEADER_ENTRY_WORD)))
        } else {
            None
        }
    }
}

/// Copy ELF load segments into the bootstrap user address range.
/// Each segment grants user read/write access to every section it touches;
/// ELF permission flags and entry-point bounds are not enforced here.
/// A later rejected segment leaves earlier mappings and copies in place.
///
/// Safety: the caller must provide exclusive access to the boot table and
/// destination RAM, ensure the input does not overlap it, and arrange instruction
/// cache synchronization before entry when required. This helper does not do it.
pub unsafe fn load_elf(bytes: &[u8]) -> Option<u32> {
    let elf = Elf32::parse(bytes)?;
    let loaded = elf.load_segments(|segment| {
        let Some(end) = segment.virtual_address.checked_add(segment.memory_size) else {
            return false;
        };
        if segment.virtual_address < USER_IMAGE_START || end > USER_STACK_TOP - STACK_RESERVED_BYTES
        {
            return false;
        }

        crate::memory::allow_user_range(segment.virtual_address, segment.memory_size);
        let destination = segment.virtual_address as *mut u8;
        // ELF memory size includes zero-initialized storage such as .bss.
        // Clear the whole region, then copy the bytes actually present in the file.
        core::ptr::write_bytes(destination, 0, segment.memory_size as usize);
        for (offset, byte) in segment.data.iter().enumerate() {
            core::ptr::write_volatile(destination.add(offset), *byte);
        }
        true
    });
    loaded.then_some(elf.entry())
}

/// Start ARM user instructions using the current address space.
/// Safety: entry must contain executable ARM code, stack must be a valid user
/// stack, and exception vectors/kernel stacks must be ready for enabled IRQs.
/// This initializes SP_usr and CPSR; other user registers retain current values.
#[unsafe(naked)]
pub unsafe extern "C" fn enter(entry: u32, stack: u32) -> ! {
    naked_asm!(
        // r0=entry and r1=stack under the ARM calling convention.
        "cpsid i",
        // System mode shares the user SP bank, so this sets SP_usr.
        "cps #{system_mode}",
        "mov sp, r1",
        // Supervisor mode supplies SPSR_svc for the exception-return operation.
        "cps #{supervisor_mode}",
        "mov r2, #{user_status}",
        "msr spsr_cxsf, r2",
        "mov lr, r0",
        // Load PC from LR and CPSR from SPSR_svc together, entering user mode.
        "movs pc, lr",
        system_mode = const CPSR_SYSTEM_MODE,
        supervisor_mode = const CPSR_SUPERVISOR_MODE,
        user_status = const INITIAL_USER_CPSR,
    );
}
