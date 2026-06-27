//! User-mode entry — switch from kernel (ring 0) to user (ring 3) via iretq.
//!
//! Slice 3.6.b: allocates a user code page + user stack, copies a tiny
//! hand-written machine code program into the code page, then builds
//! the iret frame and jumps to user mode.
//!
//! Slice 3.6.c update: the user program now makes a syscall (SYS_WRITE)
//! instead of doing direct memory writes. The syscall entry (src/syscall.rs)
//! catches it, calls the Rust handler, and writes "hello\n" to the serial
//! port on the kernel's behalf.

use crate::frame_allocator;
use crate::memory;
use crate::println;
use x86_64::structures::paging::PageTableFlags;
use x86_64::{PhysAddr, VirtAddr};

/// Virtual address where the user program's code is loaded.
pub(crate) const USER_CODE_ADDR: u64 = 0x4000_0000;

/// Virtual address range for the user stack: [USER_STACK_BOTTOM, USER_STACK_TOP).
/// 64 KB total (16 pages), plenty for a tiny program.
pub(crate) const USER_STACK_BOTTOM: u64 = 0x7000_0000;
pub(crate) const USER_STACK_TOP: u64 = USER_STACK_BOTTOM + 0x1_0000;

/// Initial RFLAGS for a freshly-exec'd user program. Same value `enter_user_mode`
/// uses for the initial program: reserved bit 1 (always 1) + bit 9 (IF, interrupt
/// enable). 0x202 = 0b0010_0000_0010.
pub(crate) const USER_RFLAGS_INIT: u64 = 0x202;

// ============================================================
// Programs registry (slice 4.5)
// ============================================================

/// The "hello" program — what `exec("hello")` loads.
///
/// Writes "hello from exec\n" to stdout, then exits 0.
///
/// Layout:
///   0x00: mov rax, 1              ; SYS_WRITE
///   0x0A: mov rdi, 0x40000031     ; "hello from exec\n"
///   0x14: mov rsi, 16             ; length
///   0x1E: syscall
///   0x20: mov rax, 60             ; SYS_EXIT
///   0x2A: xor rdi, rdi
///   0x2D: syscall
///   0x2F: jmp -2                  ; safety net
///   0x31: "hello from exec\n"     ; 16 bytes
const HELLO_PROGRAM: [u8; 65] = [
    // mov rax, 1 (SYS_WRITE)
    0x48, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // mov rdi, 0x40000031 (string address)
    0x48, 0xBF, 0x31, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // mov rsi, 16 (length of "hello from exec\n")
    0x48, 0xBE, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // syscall
    0x0F, 0x05,
    // mov rax, 60 (SYS_EXIT)
    0x48, 0xB8, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // xor rdi, rdi
    0x48, 0x31, 0xFF,
    // syscall
    0x0F, 0x05,
    // jmp -2 (safety net)
    0xEB, 0xFE,
    // 0x31: "hello from exec\n" (16 bytes)
    0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x66, 0x72, 0x6F, 0x6D,
    0x20, 0x65, 0x78, 0x65, 0x63, 0x0A,
];

/// The "goodbye" program — what `exec("goodbye")` loads.
///
/// Writes "goodbye from exec\n" to stdout, then exits 0.
/// Same code shape as HELLO_PROGRAM, different string and length.
const GOODBYE_PROGRAM: [u8; 67] = [
    // mov rax, 1 (SYS_WRITE)
    0x48, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // mov rdi, 0x40000031 (string address)
    0x48, 0xBF, 0x31, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // mov rsi, 18 (length of "goodbye from exec\n")
    0x48, 0xBE, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // syscall
    0x0F, 0x05,
    // mov rax, 60 (SYS_EXIT)
    0x48, 0xB8, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // xor rdi, rdi
    0x48, 0x31, 0xFF,
    // syscall
    0x0F, 0x05,
    // jmp -2 (safety net)
    0xEB, 0xFE,
    // 0x31: "goodbye from exec\n" (18 bytes)
    0x67, 0x6F, 0x6F, 0x64, 0x62, 0x79, 0x65, 0x20, 0x66, 0x72,
    0x6F, 0x6D, 0x20, 0x65, 0x78, 0x65, 0x63, 0x0A,
];

/// Registry of programs that can be loaded via `exec(name)`.
///
/// Each entry: `(name, bytes)`. The program is loaded as a flat binary
/// at USER_CODE_ADDR. Entry point = start of the bytes (= USER_CODE_ADDR
/// after mapping). No header, no relocation, no ELF.
pub(crate) static PROGRAMS: &[(&str, &[u8])] = &[
    ("hello", &HELLO_PROGRAM),
    ("goodbye", &GOODBYE_PROGRAM),
];

/// User code segment selector with RPL=3.
/// After the slice 4.5 GDT reorder: index 4 = USER_CODE = 0x20, with RPL=3 = 0x23.
const USER_CODE_SELECTOR: u16 = 0x23;

/// User data segment selector with RPL=3.
/// After the slice 4.5 GDT reorder: index 3 = USER_DATA = 0x18, with RPL=3 = 0x1B.
const USER_DATA_SELECTOR: u16 = 0x1B;

/// Initial RFLAGS value for user code: bit 1 (reserved, always 1) + bit 9
/// (IF, interrupt enable). So 0b0010_0000_0010 = 0x202.
const USER_RFLAGS: u64 = 0x202;

/// The user program in raw x86-64 machine code (116 bytes total).
///
/// Tests fork + exec + wait: parent forks, child execs "hello" (which
/// writes "hello from exec\n"), parent waits for the child to exit, then
/// writes "parent\n" and exits. The exec is the new piece — it replaces
/// the child's program entirely; the child never returns from exec.
///
/// Pseudocode:
///   pid = fork()              ; syscall 57
///   if (pid == 0) {
///     exec("hello")           ; syscall 59 — never returns on success
///   } else {
///     wait(NULL)              ; syscall 61 — blocks until child exits
///     write("parent\n")       ; syscall 1
///     exit(0)                 ; syscall 60
///   }
///
/// Disassembly (offsets from start of user code at 0x4000_0000):
///   0x00: mov rax, 57         ; SYS_FORK
///   0x0A: syscall
///   0x0C: test rax, rax       ; set ZF if rax == 0
///   0x0F: jz +0x3E            ; if zero (child), jump to child path at 0x4F
///   --- parent path ---
///   0x11: mov rax, 61         ; SYS_WAIT4
///   0x1B: xor rdi, rdi        ; NULL
///   0x1E: syscall             ; (blocks until child exits)
///   0x20: mov rax, 1          ; SYS_WRITE
///   0x2A: mov rdi, 0x4000006D ; "parent\n"
///   0x34: mov rsi, 7
///   0x3E: syscall
///   0x40: mov rax, 60         ; SYS_EXIT
///   0x4A: xor rdi, rdi
///   0x4D: syscall
///   --- child path ---
///   0x4F: mov rax, 59         ; SYS_EXECVE
///   0x59: mov rdi, 0x40000067 ; "hello\0"
///   0x63: syscall             ; doesn't return on success
///   0x65: jmp -2              ; safety net (only reached if exec fails)
///   --- data ---
///   0x67: "hello\0"
///   0x6D: "parent\n"
const USER_PROGRAM: [u8; 116] = [
    // 0x00: mov rax, 57 (SYS_FORK)
    0x48, 0xB8, 0x39, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x0A: syscall
    0x0F, 0x05,
    // 0x0C: test rax, rax (sets ZF if rax == 0)
    0x48, 0x85, 0xC0,
    // 0x0F: jz +0x3E (jump to child path at 0x4F; 0x11 + 0x3E = 0x4F)
    0x74, 0x3E,
    // === parent path ===
    // 0x11: mov rax, 61 (SYS_WAIT4)
    0x48, 0xB8, 0x3D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x1B: xor rdi, rdi (NULL)
    0x48, 0x31, 0xFF,
    // 0x1E: syscall (blocks until child exits)
    0x0F, 0x05,
    // 0x20: mov rax, 1 (SYS_WRITE)
    0x48, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x2A: mov rdi, 0x4000006D (parent message address)
    0x48, 0xBF, 0x6D, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0x34: mov rsi, 7 (length of "parent\n")
    0x48, 0xBE, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x3E: syscall
    0x0F, 0x05,
    // 0x40: mov rax, 60 (SYS_EXIT)
    0x48, 0xB8, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x4A: xor rdi, rdi (exit code 0)
    0x48, 0x31, 0xFF,
    // 0x4D: syscall
    0x0F, 0x05,
    // === child path ===
    // 0x4F: mov rax, 59 (SYS_EXECVE)
    0x48, 0xB8, 0x3B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x59: mov rdi, 0x40000067 (program name "hello\0")
    0x48, 0xBF, 0x67, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0x63: syscall (never returns on success)
    0x0F, 0x05,
    // 0x65: jmp -2 (safety net — only reached if exec failed)
    0xEB, 0xFE,
    // === data ===
    // 0x67: "hello\0" (6 bytes)
    0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x00,
    // 0x6D: "parent\n" (7 bytes)
    0x70, 0x61, 0x72, 0x65, 0x6E, 0x74, 0x0A,
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
            "push 0x1B",            // SS (user data | RPL=3) — slice 4.5 GDT reorder
            "push 0x70010000",      // RSP (top of user stack)
            "push 0x202",           // RFLAGS (reserved bit 1 + interrupt-enable bit)
            "push 0x23",            // CS (user code | RPL=3) — slice 4.5 GDT reorder
            "push 0x40000000",      // RIP (user program entry)
            "iretq",                // pop 5 values, jump to user code at ring 3
            options(nostack, nomem, preserves_flags),
        );
    }
    // iretq transfers control to user code; this point is unreachable.
    unreachable!("iretq should have transferred control to user mode");
}
