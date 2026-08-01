use core::arch::naked_asm;

const USER_HEADER: *const u32 = 0x4100_0000 as *const u32;
const USER_MAGIC: u32 = 0x5849_4247;
pub const USER_STACK_TOP: u32 = 0x47df_f000;

pub fn loaded_entry() -> Option<u32> {
    unsafe {
        if core::ptr::read_volatile(USER_HEADER) == USER_MAGIC {
            Some(core::ptr::read_volatile(USER_HEADER.add(1)))
        } else {
            None
        }
    }
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
