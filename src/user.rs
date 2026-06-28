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
/// Writes "hello from exec\n" to stdout (fd=1), then exits 0.
///
/// Slice 2.5 update: switched to Linux convention `sys_write(fd, buf, len)`.
/// Each write syscall now sets rdi=1 (fd=1) before the buffer address in rsi
/// and length in rdx.
///
/// Layout:
///   0x00: mov rax, 1              ; SYS_WRITE
///   0x0A: mov rdi, 1              ; fd=1 (stdout)
///   0x14: mov rsi, 0x4000003B     ; "hello from exec\n"
///   0x1E: mov rdx, 16             ; length
///   0x28: syscall
///   0x2A: mov rax, 60             ; SYS_EXIT
///   0x34: xor rdi, rdi
///   0x37: syscall
///   0x39: jmp -2                  ; safety net
///   0x3B: "hello from exec\n"     ; 16 bytes
const HELLO_PROGRAM: [u8; 75] = [
    // mov rax, 1 (SYS_WRITE)
    0x48, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // mov rdi, 1 (fd=1, stdout)
    0x48, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // mov rsi, 0x4000003B (string address)
    0x48, 0xBE, 0x3B, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // mov rdx, 16 (length of "hello from exec\n")
    0x48, 0xBA, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
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
    // 0x3B: "hello from exec\n" (16 bytes)
    0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x66, 0x72, 0x6F, 0x6D,
    0x20, 0x65, 0x78, 0x65, 0x63, 0x0A,
];

/// The "goodbye" program — what `exec("goodbye")` loads.
///
/// Writes "goodbye from exec\n" to stdout (fd=1), then exits 0.
/// Same code shape as HELLO_PROGRAM, different string and length.
const GOODBYE_PROGRAM: [u8; 77] = [
    // mov rax, 1 (SYS_WRITE)
    0x48, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // mov rdi, 1 (fd=1, stdout)
    0x48, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // mov rsi, 0x4000003B (string address)
    0x48, 0xBE, 0x3B, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // mov rdx, 18 (length of "goodbye from exec\n")
    0x48, 0xBA, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
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
    // 0x3B: "goodbye from exec\n" (18 bytes)
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

/// The user program in raw x86-64 machine code (133 bytes total).
///
/// Lesson 2 step 2.6: tests the four VFS syscalls end-to-end against the
/// hardcoded inode (`HARDCODED_INODE`, which contains "hello vfs\n").
///
/// Pseudocode:
///   fd = open(NULL)            ; syscall 2 (path ignored in Lesson 2)
///   n = read(fd, buf, 10)      ; syscall 0 — reads "hello vfs\n"
///   write(1, buf, 10)          ; syscall 1 — prints "hello vfs\n" to stdout
///   close(fd)                  ; syscall 3
///   exit(0)                    ; syscall 60
///
/// Output should show: `hello vfs` followed by a newline, plus the kernel
/// trace lines for each syscall.
///
/// Disassembly (offsets from start of user code at 0x4000_0000):
///   0x00: mov rax, 2           ; SYS_OPEN
///   0x0A: xor rdi, rdi         ; path = NULL (kernel ignores)
///   0x0D: syscall              ; returns fd=0 (first open in fresh table)
///   0x0F: xor rdi, rdi         ; fd = 0
///   0x12: mov rsi, 0x40000075  ; BUF_OUT (16 bytes after the code)
///   0x1C: mov rdx, 10          ; length = 10
///   0x26: xor rax, rax         ; SYS_READ
///   0x29: syscall              ; reads "hello vfs\n" into BUF_OUT
///   0x2B: mov rdi, 1           ; fd = 1 (stdout)
///   0x35: mov rsi, 0x40000075  ; BUF_OUT (same buffer)
///   0x3F: mov rdx, 10          ; length = 10
///   0x49: mov rax, 1           ; SYS_WRITE
///   0x53: syscall              ; prints "hello vfs\n" to serial
///   0x55: xor rdi, rdi         ; fd = 0
///   0x58: mov rax, 3           ; SYS_CLOSE
///   0x62: syscall              ; closes fd=0
///   0x64: xor rdi, rdi         ; exit code = 0
///   0x67: mov rax, 60          ; SYS_EXIT
///   0x71: syscall
///   0x73: jmp -2               ; safety net
///   0x75: <16-byte buffer>     ; BUF_OUT
const USER_PROGRAM: [u8; 133] = [
    // 0x00: mov rax, 2 (SYS_OPEN)
    0x48, 0xB8, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x0A: xor rdi, rdi (path = NULL)
    0x48, 0x31, 0xFF,
    // 0x0D: syscall (returns fd=0)
    0x0F, 0x05,
    // 0x0F: xor rdi, rdi (fd = 0)
    0x48, 0x31, 0xFF,
    // 0x12: mov rsi, 0x40000075 (BUF_OUT)
    0x48, 0xBE, 0x75, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0x1C: mov rdx, 10 (length)
    0x48, 0xBA, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x26: xor rax, rax (SYS_READ = 0)
    0x48, 0x31, 0xC0,
    // 0x29: syscall (reads "hello vfs\n" into BUF_OUT)
    0x0F, 0x05,
    // 0x2B: mov rdi, 1 (fd=1, stdout)
    0x48, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x35: mov rsi, 0x40000075 (BUF_OUT, same buffer)
    0x48, 0xBE, 0x75, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0x3F: mov rdx, 10 (length)
    0x48, 0xBA, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x49: mov rax, 1 (SYS_WRITE)
    0x48, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x53: syscall (prints to serial)
    0x0F, 0x05,
    // 0x55: xor rdi, rdi (fd = 0)
    0x48, 0x31, 0xFF,
    // 0x58: mov rax, 3 (SYS_CLOSE)
    0x48, 0xB8, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x62: syscall
    0x0F, 0x05,
    // 0x64: xor rdi, rdi (exit code = 0)
    0x48, 0x31, 0xFF,
    // 0x67: mov rax, 60 (SYS_EXIT)
    0x48, 0xB8, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x71: syscall
    0x0F, 0x05,
    // 0x73: jmp -2 (safety net)
    0xEB, 0xFE,
    // 0x75: 16-byte scratch buffer for read() output
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
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
