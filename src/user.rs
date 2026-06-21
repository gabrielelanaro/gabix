//! User-mode entry — switch from kernel (ring 0) to user (ring 3) via iretq.
//!
//! Slice 3.6.b: allocates a user code page + user stack, copies a tiny
//! hand-written machine code program into the code page, then builds
//! the iret frame and jumps to user mode.
//!
//! The user program:
//!   1. Writes a magic value (0xDEADBEEF) to its own writable memory
//!      at 0x7000_0000 (Option A — proves user code runs and can write
//!      to its own memory).
//!   2. Tries to write to a kernel address (0x200000) → page fault
//!      because the kernel page is supervisor-only (Option B — proves
//!      the CPU correctly blocks user accesses to kernel memory).

use crate::frame_allocator;
use crate::memory;
use crate::println;
use x86_64::structures::paging::PageTableFlags;
use x86_64::{PhysAddr, VirtAddr};

/// Virtual address where the user program's code is loaded.
const USER_CODE_ADDR: u64 = 0x4000_0000;

/// Virtual address range for the user stack: [USER_STACK_BOTTOM, USER_STACK_TOP).
/// 64 KB total (16 pages), plenty for a tiny program.
const USER_STACK_BOTTOM: u64 = 0x7000_0000;
const USER_STACK_TOP: u64 = USER_STACK_BOTTOM + 0x1_0000;

/// User code segment selector with RPL=3: GDT index 3, ring 3 = 0x18 | 3 = 0x1B
const USER_CODE_SELECTOR: u16 = 0x1B;

/// User data segment selector with RPL=3: GDT index 4, ring 3 = 0x20 | 3 = 0x23
const USER_DATA_SELECTOR: u16 = 0x23;

/// Initial RFLAGS value for user code: bit 1 (reserved, always 1) + bit 9
/// (IF, interrupt enable). So 0b0010_0000_0010 = 0x202.
const USER_RFLAGS: u64 = 0x202;

/// The user program in raw x86-64 machine code (40 bytes total).
///
/// Disassembly:
///   mov rax, 0x70000000          ; user-writable address
///   mov rcx, 0xDEADBEEF          ; magic value
///   mov [rax], rcx               ; write magic to user memory (Option A)
///   mov rax, 0x200000            ; kernel address (supervisor-only)
///   mov qword [rax], 0           ; illegal write → page fault (Option B)
const USER_PROGRAM: [u8; 40] = [
    // mov rax, 0x0000_0000_7000_0000
    0x48, 0xB8, 0x00, 0x00, 0x00, 0x70, 0x00, 0x00, 0x00, 0x00,
    // mov rcx, 0x0000_0000_DEAD_BEEF
    0x48, 0xB9, 0xEF, 0xBE, 0xAD, 0xDE, 0x00, 0x00, 0x00, 0x00,
    // mov [rax], rcx
    0x48, 0x89, 0x08,
    // mov rax, 0x0000_0000_0020_0000
    0x48, 0xB8, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00,
    // mov qword [rax], 0  (sign-extended imm32)
    0x48, 0xC7, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Transition from kernel (ring 0) to user (ring 3) and start running
/// the user program. Does not return — iretq transfers control to user
/// code, which runs until it triggers a page fault.
pub fn enter_user_mode() -> ! {
    // Disable interrupts during setup — we don't want the timer firing
    // mid-mapping. The RFLAGS value pushed in the iret frame will
    // re-enable interrupts in user mode.
    x86_64::instructions::interrupts::disable();
    println!("[user] setting up user program (3.6.b)");

    // === Allocate and map the user code page ===
    let code_frame = frame_allocator::allocate().expect("out of frames for user code");
    let code_phys = PhysAddr::new(code_frame * 4096);
    // SAFETY: virtual address is fresh (unused), frame is fresh.
    unsafe {
        memory::map_to(
            VirtAddr::new(USER_CODE_ADDR),
            code_phys,
            PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE,
        )
        .expect("map_to failed for user code");
    }

    // Copy the user program bytes into the code page.
    unsafe {
        let code_ptr = USER_CODE_ADDR as *mut u8;
        for (i, byte) in USER_PROGRAM.iter().enumerate() {
            core::ptr::write_volatile(code_ptr.add(i), *byte);
        }
    }
    println!(
        "[user] code mapped at 0x{:x} (frame {}, {} bytes)",
        USER_CODE_ADDR,
        code_frame,
        USER_PROGRAM.len()
    );

    // === Allocate and map the user stack (16 pages = 64 KB) ===
    let mut stack_frames = 0;
    let mut virt = USER_STACK_BOTTOM;
    while virt < USER_STACK_TOP {
        let frame = frame_allocator::allocate().expect("out of frames for user stack");
        let phys = PhysAddr::new(frame * 4096);
        // SAFETY: virtual range is fresh, frame is fresh.
        unsafe {
            memory::map_to(
                VirtAddr::new(virt),
                phys,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE,
            )
            .expect("map_to failed for user stack");
        }
        stack_frames += 1;
        virt += 4096;
    }
    println!(
        "[user] stack mapped at 0x{:x}-0x{:x} ({} frames)",
        USER_STACK_BOTTOM, USER_STACK_TOP, stack_frames
    );

    // === Build the iret frame on the kernel stack and jump to user mode ===
    //
    // The CPU's iretq pops 5 values from the stack: RIP, CS, RFLAGS, RSP, SS.
    // If the target CS has a different ring than the current CPL (here:
    // target ring 3 vs current ring 0), the CPU loads SS:RSP from the
    // frame (switching stacks) instead of just RIP:CS.
    //
    // We push in reverse order (last push = first pop = RIP):
    //   push SS    (user data segment | RPL=3 = 0x23)
    //   push RSP   (top of user stack)
    //   push RFLAGS (0x202: reserved bit 1 + interrupt-enable bit)
    //   push CS    (user code segment | RPL=3 = 0x1B)
    //   push RIP   (entry of user program)
    //   iretq
    println!(
        "[user] transitioning to ring 3: CS=0x{:x} RIP=0x{:x} SS=0x{:x} RSP=0x{:x}",
        USER_CODE_SELECTOR, USER_CODE_ADDR, USER_DATA_SELECTOR, USER_STACK_TOP
    );

    unsafe {
        core::arch::asm!(
            "push 0x23",            // SS (user data | RPL=3)
            "push 0x70010000",      // RSP (top of user stack)
            "push 0x202",           // RFLAGS (reserved bit 1 + interrupt-enable bit)
            "push 0x1B",            // CS (user code | RPL=3)
            "push 0x40000000",      // RIP (user program entry)
            "iretq",                // pop 5 values, jump to user code at ring 3
            options(nostack, nomem, preserves_flags),
        );
    }
    // iretq transfers control to user code; this point is unreachable.
    unreachable!("iretq should have transferred control to user mode");
}
