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

/// The user program in raw x86-64 machine code (270 bytes total).
///
/// Lesson 3 step 3.6: open two different files by path, prove they resolve
/// to different inodes with different contents. Cycle 1 opens "/hello.txt",
/// reads "hello vfs\n", prints to stdout, closes. Cycle 2 opens
/// "/secret.txt", reads "top secret\n", prints, closes. Then exits.
///
/// Pseudocode:
///   ; Cycle 1 — hello.txt (10 bytes)
///   fd = open("/hello.txt")
///   n = read(fd, buf, 10)
///   write(1, buf, 10)
///   close(fd)
///   ; Cycle 2 — secret.txt (11 bytes)
///   fd = open("/secret.txt")
///   n = read(fd, buf, 11)
///   write(1, buf, 11)
///   close(fd)
///   exit(0)
///
/// Expected serial output: "hello vfs" newline "top secret" newline, plus
/// kernel syscall trace lines.
///
/// Disassembly (offsets from start of user code at 0x4000_0000):
///   ; Cycle 1: hello.txt
///   0x00: mov rax, 2            ; SYS_OPEN
///   0x0A: mov rdi, 0x400000F7   ; path = "/hello.txt"
///   0x14: syscall               ; returns fd=0
///   0x16: xor rdi, rdi          ; fd = 0
///   0x19: mov rsi, 0x400000E7   ; BUF_OUT
///   0x23: mov rdx, 10
///   0x2D: xor rax, rax          ; SYS_READ
///   0x30: syscall
///   0x32: mov rdi, 1            ; stdout
///   0x3C: mov rsi, 0x400000E7   ; BUF_OUT
///   0x46: mov rdx, 10
///   0x50: mov rax, 1            ; SYS_WRITE
///   0x5A: syscall
///   0x5C: xor rdi, rdi          ; fd = 0
///   0x5F: mov rax, 3            ; SYS_CLOSE
///   0x69: syscall
///   ; Cycle 2: secret.txt
///   0x6B: mov rax, 2            ; SYS_OPEN
///   0x75: mov rdi, 0x40000102   ; path = "/secret.txt"
///   0x7F: syscall               ; returns fd=0 (hello closed)
///   0x81: xor rdi, rdi          ; fd = 0
///   0x84: mov rsi, 0x400000E7   ; BUF_OUT
///   0x8E: mov rdx, 11
///   0x98: xor rax, rax          ; SYS_READ
///   0x9B: syscall
///   0x9D: mov rdi, 1            ; stdout
///   0xA7: mov rsi, 0x400000E7   ; BUF_OUT
///   0xB1: mov rdx, 11
///   0xBB: mov rax, 1            ; SYS_WRITE
///   0xC5: syscall
///   0xC7: xor rdi, rdi          ; fd = 0
///   0xCA: mov rax, 3            ; SYS_CLOSE
///   0xD4: syscall
///   ; Exit
///   0xD6: xor rdi, rdi          ; exit code = 0
///   0xD9: mov rax, 60           ; SYS_EXIT
///   0xE3: syscall
///   0xE5: jmp -2                ; safety net
///   ; Data
///   0xE7: <16-byte buffer>      ; BUF_OUT
///   0xF7: "/hello.txt\0"        ; 11 bytes
///   0x102: "/secret.txt\0"      ; 12 bytes
const USER_PROGRAM: [u8; 270] = [
    // 0x00: mov rax, 2 (SYS_OPEN)
    0x48, 0xB8, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x0A: mov rdi, 0x400000F7 (path = "/hello.txt")
    0x48, 0xBF, 0xF7, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0x14: syscall (returns fd=0)
    0x0F, 0x05,
    // 0x16: xor rdi, rdi (fd = 0)
    0x48, 0x31, 0xFF,
    // 0x19: mov rsi, 0x400000E7 (BUF_OUT)
    0x48, 0xBE, 0xE7, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0x23: mov rdx, 10 (length)
    0x48, 0xBA, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x2D: xor rax, rax (SYS_READ = 0)
    0x48, 0x31, 0xC0,
    // 0x30: syscall (reads "hello vfs\n" into BUF_OUT)
    0x0F, 0x05,
    // 0x32: mov rdi, 1 (fd=1, stdout)
    0x48, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x3C: mov rsi, 0x400000E7 (BUF_OUT, same buffer)
    0x48, 0xBE, 0xE7, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0x46: mov rdx, 10 (length)
    0x48, 0xBA, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x50: mov rax, 1 (SYS_WRITE)
    0x48, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x5A: syscall (prints to serial)
    0x0F, 0x05,
    // 0x5C: xor rdi, rdi (fd = 0 for close)
    0x48, 0x31, 0xFF,
    // 0x5F: mov rax, 3 (SYS_CLOSE)
    0x48, 0xB8, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x69: syscall (closes hello)
    0x0F, 0x05,

    // 0x6B: mov rax, 2 (SYS_OPEN — second cycle)
    0x48, 0xB8, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x75: mov rdi, 0x40000102 (path = "/secret.txt")
    0x48, 0xBF, 0x02, 0x01, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0x7F: syscall (returns fd=0 again — hello freed it)
    0x0F, 0x05,
    // 0x81: xor rdi, rdi (fd = 0)
    0x48, 0x31, 0xFF,
    // 0x84: mov rsi, 0x400000E7 (BUF_OUT)
    0x48, 0xBE, 0xE7, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0x8E: mov rdx, 11 (length for "top secret\n")
    0x48, 0xBA, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0x98: xor rax, rax (SYS_READ = 0)
    0x48, 0x31, 0xC0,
    // 0x9B: syscall (reads "top secret\n" into BUF_OUT)
    0x0F, 0x05,
    // 0x9D: mov rdi, 1 (fd=1, stdout)
    0x48, 0xBF, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0xA7: mov rsi, 0x400000E7 (BUF_OUT, same buffer)
    0x48, 0xBE, 0xE7, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
    // 0xB1: mov rdx, 11 (length)
    0x48, 0xBA, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0xBB: mov rax, 1 (SYS_WRITE)
    0x48, 0xB8, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0xC5: syscall (prints to serial)
    0x0F, 0x05,
    // 0xC7: xor rdi, rdi (fd = 0 for close)
    0x48, 0x31, 0xFF,
    // 0xCA: mov rax, 3 (SYS_CLOSE)
    0x48, 0xB8, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0xD4: syscall (closes secret)
    0x0F, 0x05,

    // 0xD6: xor rdi, rdi (exit code = 0)
    0x48, 0x31, 0xFF,
    // 0xD9: mov rax, 60 (SYS_EXIT)
    0x48, 0xB8, 0x3C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0xE3: syscall
    0x0F, 0x05,
    // 0xE5: jmp -2 (safety net)
    0xEB, 0xFE,

    // 0xE7: 16-byte scratch buffer for read() output
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    // 0xF7: "/hello.txt\0" (11 bytes — first path string)
    0x2F, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x2E, 0x74, 0x78, 0x74, 0x00,
    // 0x102: "/secret.txt\0" (12 bytes — second path string)
    0x2F, 0x73, 0x65, 0x63, 0x72, 0x65, 0x74, 0x2E, 0x74, 0x78, 0x74, 0x00,
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
