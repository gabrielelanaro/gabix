use core::arch::naked_asm;
use gabix_kernel::elf::Elf32;

#[cfg(feature = "virt")]
const USER_HEADER: *const u32 = 0x4100_0000 as *const u32;
#[cfg(feature = "virt")]
const USER_MAGIC: u32 = 0x5849_4247;
const USER_IMAGE_START: u32 = 0x4100_0000;
const STACK_RESERVED_BYTES: u32 = 0x10_0000;
pub const USER_STACK_TOP: u32 = 0x47df_f000;

#[cfg(feature = "virt")]
pub fn loaded_entry() -> Option<u32> {
    unsafe {
        if core::ptr::read_volatile(USER_HEADER) == USER_MAGIC {
            Some(core::ptr::read_volatile(USER_HEADER.add(1)))
        } else {
            None
        }
    }
}

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

#[unsafe(naked)]
pub unsafe extern "C" fn enter(entry: u32, stack: u32) -> ! {
    naked_asm!(
        "cpsid i",
        "cps #0x1f",
        "mov sp, r1",
        "cps #0x13",
        "mov r2, #0x10",
        "msr spsr_cxsf, r2",
        "mov lr, r0",
        "movs pc, lr",
    );
}
