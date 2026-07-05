//! Syscall mechanism — the fast path for user-to-kernel transitions.
//!
//! Slice 3.6.c: sets up the SYSCALL/SYSRET instructions via MSRs
//! (Model-Specific Registers) and provides a naked-asm entry point
//! that switches to the kernel stack, calls the Rust handler, then
//! returns via SYSRET.
//!
//! Currently dispatches exactly one syscall: SYS_WRITE (write a buffer
//! to the serial port). User programs call it like:
//!   rax = 1 (SYS_WRITE)
//!   rdi = buffer address
//!   rsi = length
//!   syscall

use crate::frame_allocator;
use crate::fs;
use crate::memory;
use crate::println;
use crate::serial;
use alloc::sync::Arc;
use core::arch::naked_asm;
use core::sync::atomic::AtomicU64;
use x86_64::structures::paging::PageTableFlags;
use x86_64::{PhysAddr, VirtAddr};

/// MSR (Model-Specific Register) addresses for the syscall configuration.
/// These are well-known constants documented in the Intel SDM
/// (Software Developer's Manual) Volume 4.
const MSR_EFER: u32 = 0xC0000080; // Extended Feature Enable Register
const MSR_STAR: u32 = 0xC0000081; // System call target CS and SS
const MSR_LSTAR: u32 = 0xC0000082; // System call target RIP (entry point)
const MSR_FMASK: u32 = 0xC0000084; // System call RFLAGS clear mask

/// Bit 0 of EFER: SYSTEM_CALL_EXTENSIONS (SCE). Enables the SYSCALL instruction.
const EFER_SCE: u64 = 1;

/// Bit 9 of RFLAGS: IF (Interrupt Flag). We clear this on syscall entry
/// so the handler runs with interrupts disabled until we explicitly enable.
const RFLAGS_IF: u64 = 0x200;

/// Syscall numbers. Match what user programs put in RAX.
/// These follow the Linux x86-64 syscall ABI conventions, so a real
/// Linux binary's syscall numbers would (theoretically) line up —
/// modulo not actually having the right kernel-side implementations.
const SYS_READ: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 2;
const SYS_CLOSE: u64 = 3;
const SYS_GETPID: u64 = 39;
const SYS_FORK: u64 = 57;
const SYS_EXECVE: u64 = 59;
const SYS_EXIT: u64 = 60;
const SYS_WAIT4: u64 = 61;

/// Virtual address where the dedicated syscall stack begins.
/// Picked to be far from everything else (kernel image, heap, task
/// stacks, user code/stack).
pub(crate) const SYSCALL_STACK_BOTTOM: u64 = 0x6000_0000_0000;
/// 16 KB syscall stack (4 frames) — matches Linux's kernel stack size.
pub(crate) const SYSCALL_STACK_SIZE: u64 = 16 * 4096;
pub(crate) const SYSCALL_STACK_TOP: u64 = SYSCALL_STACK_BOTTOM + SYSCALL_STACK_SIZE;

/// Saved user state, captured by `syscall_entry` on every syscall.
///
/// These are needed by `sys_fork`: it clones the current process by
/// snapshotting the user-mode register state at the moment the syscall
/// fired, then constructing a child task whose saved state matches.
///
/// The alternative — fishing these values out of the parent's kernel
/// stack frames from inside `sys_fork` — would require knowing the exact
/// stack layout at the time `sys_fork` runs, which is fragile. Global
/// atomics are simpler.
///
/// Only the registers `sys_fork` actually needs are saved here:
///   - user RIP (where to resume in user code — was in RCX)
///   - user RSP (where the user stack pointer was)
///   - user RFLAGS (was in R11)
///   - the 6 callee-saved registers (RBX, RBP, R12-R15) — these MUST be
///     preserved across the syscall per the System V AMD64 ABI, so the
///     child needs the parent's values to look like a true copy.
///
/// Caller-saved registers (RAX, RCX, RDX, RSI, RDI, R8, R9, R10, R11) are
/// NOT saved here: the calling convention allows them to be clobbered
/// across a syscall, so the parent's compiler already treats them as
/// garbage after the syscall. The child gets the same treatment.
pub(crate) static USER_RIP_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_RFLAGS_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_RSP_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_RBX_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_RBP_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_R12_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_R13_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_R14_SAVE: AtomicU64 = AtomicU64::new(0);
pub(crate) static USER_R15_SAVE: AtomicU64 = AtomicU64::new(0);

/// Public read accessors for the saved user state. Used by `fork_trampoline`
/// (in task.rs) to load the values into the right registers for `sysretq`.
// (RIP/RSP/RFLAGS could be pub(crate) too, but keeping them private behind
// accessors makes the API boundary clearer.)
pub(crate) fn saved_user_rip() -> u64 {
    USER_RIP_SAVE.load(core::sync::atomic::Ordering::SeqCst)
}
pub(crate) fn saved_user_rsp() -> u64 {
    USER_RSP_SAVE.load(core::sync::atomic::Ordering::SeqCst)
}
pub(crate) fn saved_user_rflags() -> u64 {
    USER_RFLAGS_SAVE.load(core::sync::atomic::Ordering::SeqCst)
}

/// Kernel RSP to switch to on syscall entry. Set during init to the top
/// of a dedicated 16 KB stack region with room below for handler frames.
///
/// Updated by `task::yield_now` on every context switch to point at the
/// NEW current task's `entry_stack_top`. This is what makes per-task
/// entry stacks work: syscall_entry reads this value, and TSS.RSP0 is
/// kept in sync for user-mode timer interrupts.
static KERNEL_RSP: AtomicU64 = AtomicU64::new(0);

/// Set the kernel-side RSP that `syscall_entry` switches to. Called by
/// `task::yield_now` when switching to a new task. The new task's
/// `entry_stack_top` becomes the current entry stack.
pub(crate) fn set_kernel_rsp(rsp: u64) {
    KERNEL_RSP.store(rsp, core::sync::atomic::Ordering::SeqCst);
}

/// Initialize the syscall mechanism: allocate a dedicated kernel stack,
/// set up the MSRs, and save the kernel stack pointer. Must be called
/// from kernel code, before any user program runs.
pub fn init() {
    // Allocate and map a dedicated 16 KB stack for syscall entry.
    // Using a fresh stack (instead of the captured mid-stack RSP) means
    // we always have plenty of room for handler frames.
    let mut frames_allocated = 0;
    let mut virt = SYSCALL_STACK_BOTTOM;
    while virt < SYSCALL_STACK_TOP {
        let frame = frame_allocator::allocate().expect("out of frames for syscall stack");
        let phys = PhysAddr::new(frame * 4096);
        // SAFETY: virtual range is fresh (unused), frame is fresh.
        unsafe {
            memory::map_to(
                VirtAddr::new(virt),
                phys,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
            )
            .expect("map_to failed for syscall stack");
        }
        frames_allocated += 1;
        virt += 4096;
    }
    // KERNEL_RSP = top of the stack region (stack grows DOWN, so the
    // top is where we start).
    KERNEL_RSP.store(SYSCALL_STACK_TOP, core::sync::atomic::Ordering::SeqCst);

    // Mirror it into TSS.RSP0 so the FIRST timer fire in user mode lands
    // on the syscall stack (not the boot stack captured at gdt::init).
    // Without this, the first preempted user task's saved context.rsp
    // ends up pointing at the boot stack — and the subsequent switch-back
    // resumes on the boot stack, which may have been clobbered by then.
    // After the first yield_now, TSS.RSP0 stays in sync with KERNEL_RSP.
    crate::gdt::set_rsp0(SYSCALL_STACK_TOP);

    unsafe {
        // Enable the SYSCALL instruction (EFER.SCE = 1).
        let efer = rdmsr(MSR_EFER);
        wrmsr(MSR_EFER, efer | EFER_SCE);

        // STAR: kernel CS base in bits 32-47, user CS base in bits 48-63.
        // On SYSCALL: CPU loads CS = STAR[32:47], SS = STAR[32:47] + 8.
        // On SYSRET:  CPU loads CS = STAR[48:63] + 16, SS = STAR[48:63] + 8
        //             (both with RPL=3 added on AMD hardware).
        //
        // Our GDT layout (after the fix in slice 4.5):
        //   index 1 (sel 0x08): kernel code
        //   index 2 (sel 0x10): kernel data
        //   index 3 (sel 0x18): USER DATA   ← (swapped from previous order)
        //   index 4 (sel 0x20): USER CODE
        //   index 5 (sel 0x28): TSS
        //
        // With STAR[32:47] = 0x08 and STAR[48:63] = 0x10:
        //   SYSCALL: CS = 0x08, SS = 0x10 ✓
        //   SYSRET:  CS = 0x10 + 16 = 0x20 | RPL=3 = 0x23 (USER CODE) ✓
        //            SS = 0x10 + 8  = 0x18 | RPL=3 = 0x1B (USER DATA) ✓
        //
        // Previously STAR[48:63] was 0x08, which made SYSRET compute
        // SS = 0x10 (kernel data). sysretq itself doesn't check DPL in
        // long mode, so user code ran with cached SS pointing at kernel
        // data — but the next iretq (e.g., timer firing on user mode)
        // checked DPL ≠ CPL and #GP'd. Fixed by swapping USER_CS / USER_DS
        // in the GDT and setting STAR[48:63] = 0x10.
        wrmsr(MSR_STAR, 0x0010_0008_0000_0000);

        // LSTAR: address of the syscall entry point.
        wrmsr(MSR_LSTAR, syscall_entry as *const () as u64);

        // FMASK: bits to clear from RFLAGS on syscall entry. We clear IF
        // so the entry runs with interrupts disabled.
        wrmsr(MSR_FMASK, RFLAGS_IF);
    }

    println!(
        "[syscall] initialized: dedicated stack 0x{:x}-0x{:x} ({} frames), entry=0x{:x}",
        SYSCALL_STACK_BOTTOM,
        SYSCALL_STACK_TOP,
        frames_allocated,
        syscall_entry as *const () as u64
    );
}

/// Read a 64-bit MSR (Model-Specific Register).
unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    core::arch::asm!(
        "rdmsr",
        out("eax") low,
        out("edx") high,
        in("ecx") msr,
        options(nostack, nomem),
    );
    ((high as u64) << 32) | (low as u64)
}

/// Write a 64-bit value to an MSR.
unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nostack, nomem),
    );
}

/// Naked syscall entry point. The CPU jumps here when user code executes
/// the `syscall` instruction.
///
/// On entry, the CPU has already done some work for us:
///   - Saved user RIP into RCX (so we can return via SYSRET)
///   - Saved user RFLAGS into R11
///   - Loaded CS from STAR[32:47] (we're now ring 0)
///   - Cleared the bits in RFLAGS that FMASK specifies (we cleared IF)
///
/// We still need to:
///   - Switch from the user stack to the kernel stack
///   - Save callee-saved registers (the Rust handler will preserve them
///     anyway, but we also need RAX preserved across the call, and we
///     don't have a free caller-saved slot — so we save everything we
///     care about)
///   - Enable interrupts (the handler can be preempted)
///   - Call the Rust handler
///   - Restore everything and SYSRET back
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    naked_asm!(
        // Step 1: Switch to kernel stack. Use r10 as scratch (caller-saved,
        // we don't care about its entry value).
        "mov r10, rsp",                    // save user RSP in r10
        "mov rsp, [{kernel_rsp}]",         // switch to kernel stack
        "push r10",                        // push user RSP onto kernel stack

        // Step 2: Push callee-saved registers (handler might clobber them
        // if it doesn't follow the convention, and we want them back).
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // Step 3: Push user state for SYSRET.
        "push rcx",                        // user RIP
        "push r11",                        // user RFLAGS

        // Step 3.5: Snapshot user state to globals so sys_fork can read it.
        // All the values we need are still in their original registers
        // (we just pushed copies above). We use R8 as scratch — it's
        // caller-saved, not used for syscall args 1-4, and we don't use
        // args 5-6 in our kernel. Doing this BEFORE step 4 is important:
        // step 4 clobbers RCX (writes the user's arg3 there), so we have
        // to grab user RIP from RCX first.
        "mov r8, r10",
        "mov [{user_rsp_save}], r8",
        "mov r8, rcx",
        "mov [{user_rip_save}], r8",
        "mov r8, r11",
        "mov [{user_rflags_save}], r8",
        "mov r8, rbx",
        "mov [{user_rbx_save}], r8",
        "mov r8, rbp",
        "mov [{user_rbp_save}], r8",
        "mov r8, r12",
        "mov [{user_r12_save}], r8",
        "mov r8, r13",
        "mov [{user_r13_save}], r8",
        "mov r8, r14",
        "mov [{user_r14_save}], r8",
        "mov r8, r15",
        "mov [{user_r15_save}], r8",

        // Step 4: Set up System V AMD64 calling convention for the Rust
        // handler. Move args from syscall positions to System V positions.
        "mov rcx, rdx",                    // System V arg4 = rdx (was syscall arg3)
        "mov rdx, rsi",                    // System V arg3 = rsi (was syscall arg2)
        "mov rsi, rdi",                    // System V arg2 = rdi (was syscall arg1)
        "mov rdi, rax",                    // System V arg1 = rax (was syscall number)
        // r8, r9 unchanged (args 5, 6 if we ever need them — well, r8 was
        // scratch above, but we don't use args 5-6 yet so no value lost)

        // Step 5: Realign the stack to 16 bytes for the call. We pushed
        // 9 things (1 + 6 + 2) = 72 bytes, which is 8 mod 16. Subtract
        // 8 more to make it 0 mod 16 for the System V ABI.
        "sub rsp, 8",

        // Step 6: Enable interrupts in the kernel.
        "sti",

        // Step 7: Call the Rust handler. Return value lands in RAX.
        "call {handler}",

        // Step 8: Disable interrupts before returning to user.
        "cli",

        // Step 9: Undo the alignment padding.
        "add rsp, 8",

        // Step 10: Save the return value in r10 (caller-saved, won't be popped).
        "mov r10, rax",

        // Step 11: Pop user state.
        "pop r11",                         // user RFLAGS
        "pop rcx",                         // user RIP

        // Step 12: Pop callee-saved registers.
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",

        // Step 13: Restore user RSP (pop the value we pushed in step 1).
        "pop rsp",

        // Step 14: Restore the syscall return value.
        "mov rax, r10",

        // Step 15: Return to user. SYSRET uses RCX=RIP, R11=RFLAGS, RAX=return.
        "sysretq",

        kernel_rsp = sym KERNEL_RSP,
        handler = sym syscall_handler,
        user_rsp_save = sym USER_RSP_SAVE,
        user_rip_save = sym USER_RIP_SAVE,
        user_rflags_save = sym USER_RFLAGS_SAVE,
        user_rbx_save = sym USER_RBX_SAVE,
        user_rbp_save = sym USER_RBP_SAVE,
        user_r12_save = sym USER_R12_SAVE,
        user_r13_save = sym USER_R13_SAVE,
        user_r14_save = sym USER_R14_SAVE,
        user_r15_save = sym USER_R15_SAVE,
    );
}

extern "C" fn syscall_handler(num: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    println!("[syscall] num={} args=({}, {}, {}) pid={}", num, arg1, arg2, arg3, crate::task::current_pid());
    match num {
        SYS_READ => sys_read(arg1, arg2, arg3),
        SYS_WRITE => sys_write(arg1, arg2, arg3),
        SYS_OPEN => sys_open(arg1),
        SYS_CLOSE => sys_close(arg1),
        SYS_GETPID => sys_getpid(),
        SYS_FORK => sys_fork(),
        SYS_EXECVE => sys_exec(arg1),
        SYS_EXIT => sys_exit(arg1),
        SYS_WAIT4 => sys_wait(arg1, arg2),
        _ => u64::MAX,
    }
}

// ============================================================
// User-pointer validation
// ============================================================

/// Maximum user-space virtual address (exclusive).
///
/// On x86-64 with canonical addressing, user space is the lower canonical
/// half: addresses with bits 47-63 all zero, i.e., `0` through `2^47 - 1`.
/// The address `2^47` itself is non-canonical (bit 47 is 1 but bits 48-63
/// aren't all 1).
///
/// Any pointer at or above this is either in the non-canonical hole or in
/// the kernel upper half — either way, not a valid user pointer.
const USER_ADDR_LIMIT: u64 = 0x0000_8000_0000_0000;

/// Return `true` if `[ptr, ptr+len)` is entirely within user space.
///
/// Used by syscall handlers to reject kernel pointers before dereferencing
/// them. Without this, a user program could pass e.g. `0xFFFFFFFF80000000`
/// (kernel .text) to `sys_read` and have the kernel memcpy its own memory
/// into the user's buffer.
///
/// Catches the *range* attack but not the *faulting* case: a pointer in
/// range but unmapped still page-faults during copy, and without a page-
/// fault handler that triple-faults the kernel. The fault-tolerant version
/// (`copy_to_user` with exception tables) requires a page-fault handler
/// — deferred until that lands.
fn is_user_range(ptr: u64, len: u64) -> bool {
    if ptr >= USER_ADDR_LIMIT {
        return false;
    }
    // ptr + len must not overflow u64 and must stay in user space.
    match ptr.checked_add(len) {
        Some(end) => end <= USER_ADDR_LIMIT,
        None => false,
    }
}

/// Read a null-terminated path string from user memory.
///
/// Maximum length is 256 bytes (Linux's `PATH_MAX` is 4096, but we don't have
/// long paths in mini-os). Returns `None` on:
///   - NULL pointer (`ptr == 0`)
///   - Pointer outside user address space (`is_user_range` fails)
///   - No null terminator within 256 bytes
///   - Invalid UTF-8 in the bytes before the null (mini-os paths are UTF-8;
///     Linux paths are raw byte strings, a generality we don't need yet)
///
/// **Known limitation:** a pointer that's in user range but unmapped still
/// page-faults during the read. Without a page-fault handler that triple-
/// faults the kernel. See learning-record 0005 for the broader pattern.
fn read_user_path(ptr: u64) -> Option<alloc::string::String> {
    const MAX_PATH: usize = 256;

    if ptr == 0 {
        return None;
    }
    if !is_user_range(ptr, MAX_PATH as u64) {
        return None;
    }

    // SAFETY: `ptr` is non-NULL and inside user range. We read at most
    // MAX_PATH bytes. The in-range-but-unmapped case is a known limitation
    // documented above.
    let bytes: &[u8] = unsafe { core::slice::from_raw_parts(ptr as *const u8, MAX_PATH) };
    let end = bytes.iter().position(|&b| b == 0)?;
    if end == 0 {
        return None; // empty path
    }

    let s = core::str::from_utf8(&bytes[..end]).ok()?;
    Some(alloc::string::String::from(s))
}

/// SYS_READ: read up to `length` bytes from the file at `fd` into the user
/// buffer at `buffer_ptr`.
///
/// Returns the number of bytes actually read. Returns 0 at end-of-file.
/// Returns `u64::MAX` (-1 cast) on bad FD.
///
/// **User-buffer access (Lesson 2 simplification):** we cast `buffer_ptr`
/// directly to a kernel pointer. This works because the syscall handler
/// runs with the user's CR3 still loaded — the kernel mappings (PML4
/// entries 256-511) coexist with the user mappings (entries 0-255) in the
/// same page tables, so both address spaces are reachable without a
/// switch. Lesson 5+ will add `copy_to_user` / `copy_from_user` helpers
/// that validate the pointer is in user range and survive page faults
/// (real Linux's pattern).
fn sys_read(fd: u64, buffer_ptr: u64, length: u64) -> u64 {
    if !is_user_range(buffer_ptr, length) {
        println!(
            "[syscall] sys_read: bad user pointer buf=0x{:x} len={}",
            buffer_ptr, length
        );
        return u64::MAX; // EFAULT — caller tried to read/write kernel memory
    }

    // Pull the File out of the FdTable. The closure form clones the Arc
    // out — we hold our own Arc<File> and the FdTable lock can release
    // immediately, before we touch the inode's data.
    let file = match crate::task::with_current_fd_table(|fdt| fdt.get(fd)) {
        Some(f) => f,
        None => {
            println!("[syscall] sys_read: bad fd={}", fd);
            return u64::MAX;
        }
    };

    // Lock order: inode data first, then cursor. Always this order to
    // avoid deadlock if anyone else ever locks both.
    let data = file.inode.data.lock();
    let mut cursor = file.cursor.lock();

    let cursor_pos = *cursor as usize;
    let available = data.len().saturating_sub(cursor_pos);
    let to_read = (length as usize).min(available);

    // Guard the unsafe so we don't form a pointer past the end of `data`
    // even when `to_read == 0` (which would be UB even without deref).
    if to_read > 0 {
        // SAFETY: `buffer_ptr` is a user virtual address; user mappings
        // are visible to us. `data` is a valid Vec; `cursor_pos +
        // to_read <= data.len()` by construction above.
        unsafe {
            let src = data.as_ptr().add(cursor_pos);
            let dst = buffer_ptr as *mut u8;
            core::ptr::copy_nonoverlapping(src, dst, to_read);
        }
    }

    *cursor += to_read as u64;
    println!(
        "[syscall] sys_read fd={} buf=0x{:x} len={} -> read {} bytes (inode {} bytes, cursor {}->{})",
        fd, buffer_ptr, length, to_read, data.len(), cursor_pos, *cursor
    );

    to_read as u64
}

/// SYS_WRITE: write `length` bytes from the user buffer at `buffer_ptr` to
/// the file (or stdio stream) at `fd`.
///
/// Returns the number of bytes written.
///
/// **Dispatch by fd:**
/// - FD 1 (stdout) and FD 2 (stderr) — write to the serial port. This is
///   the special stdio case from Lesson 2's design Option 2 (reserve 0/1/2
///   at the FD-table level for stdio; keep the serial path special-cased
///   here). Real Files eventually replace this (Lesson 5 design Option 3).
/// - Any other FD — look up the File, extend its inode's data with the
///   bytes, advance the cursor.
///
/// Returns `u64::MAX` (-1) on bad FD, bad user pointer, or other failure.
fn sys_write(fd: u64, buffer_ptr: u64, length: u64) -> u64 {
    if !is_user_range(buffer_ptr, length) {
        println!(
            "[syscall] sys_write: bad user pointer buf=0x{:x} len={}",
            buffer_ptr, length
        );
        return u64::MAX; // EFAULT
    }

    // Stdio special case: FD 1 (stdout) and FD 2 (stderr) → serial.
    if fd == 1 || fd == 2 {
        let buffer =
            unsafe { core::slice::from_raw_parts(buffer_ptr as *const u8, length as usize) };
        for &byte in buffer {
            serial::SERIAL1.lock().write_byte(byte);
        }
        println!("[syscall] sys_write fd={} -> serial, {} bytes", fd, length);
        return length;
    }

    // Regular file write: look up the File, extend its inode data, advance cursor.
    let file = match crate::task::with_current_fd_table(|fdt| fdt.get(fd)) {
        Some(f) => f,
        None => {
            println!("[syscall] sys_write: bad fd={}", fd);
            return u64::MAX;
        }
    };

    // Lock order: inode data first, then cursor (same as sys_read).
    let mut data = file.inode.data.lock();
    let mut cursor = file.cursor.lock();

    let cursor_pos = *cursor as usize;
    let new_end = cursor_pos
        .checked_add(length as usize)
        .unwrap_or(data.len());

    // Extend the Vec if writing past end. New slots default to 0 — sparse
    // writes that skip the end get zero-fill, which matches POSIX behavior
    // for the file size growing past the last write.
    if new_end > data.len() {
        data.resize(new_end, 0);
    }

    // SAFETY: buffer_ptr is in user space (validated above). data has at
    // least new_end bytes (we resized if needed). [cursor_pos, cursor_pos+length)
    // is in-bounds by construction.
    unsafe {
        let src = buffer_ptr as *const u8;
        let dst = data[cursor_pos..].as_mut_ptr();
        core::ptr::copy_nonoverlapping(src, dst, length as usize);
    }

    *cursor += length;
    println!(
        "[syscall] sys_write fd={} -> file, {} bytes (inode now {} bytes, cursor {}->{})",
        fd, length, data.len(), cursor_pos, *cursor
    );

    length
}

/// SYS_CLOSE: release the file descriptor `fd`.
///
/// Returns 0 on success, `u64::MAX` (-1) on bad FD. Closing an already-
/// closed FD is an error (matches POSIX). The underlying File (and its
/// inode, if this was the last reference) gets dropped when the Arc's
/// refcount hits zero — for our hardcoded inode, it never does.
fn sys_close(fd: u64) -> u64 {
    let ok = crate::task::with_current_fd_table(|fdt| fdt.close(fd));
    println!("[syscall] sys_close fd={} -> {}", fd, if ok { "ok" } else { "bad" });
    if ok { 0 } else { u64::MAX }
}

/// SYS_OPEN: open a file by absolute path, return a file descriptor (FD).
///
/// Lesson 3 version: actually parses `name_ptr` as a null-terminated path
/// and walks the directory tree to resolve it. Returns `u64::MAX` (-1) on
/// any failure: bad pointer, relative path (not supported yet), missing
/// file, intermediate component not a directory.
///
/// On success: wraps the resolved `Arc<Inode>` in a fresh `File` (cursor=0),
/// inserts into the current task's FD table, returns the FD number (lowest
/// free, starting at 0 — Lesson 5 will reserve 0/1/2 for stdio and start
/// file FDs at 3).
fn sys_open(name_ptr: u64) -> u64 {
    let path = match read_user_path(name_ptr) {
        Some(p) => p,
        None => {
            println!(
                "[syscall] sys_open: bad path pointer 0x{:x}",
                name_ptr
            );
            return u64::MAX;
        }
    };

    let inode = match fs::resolve_path(&path) {
        Some(i) => i,
        None => {
            println!("[syscall] sys_open: no such file: {:?}", path);
            return u64::MAX;
        }
    };

    let file = Arc::new(fs::File::new(inode));
    let fd = crate::task::with_current_fd_table(|fdt| fdt.open(file));
    println!("[syscall] sys_open(path={:?}) -> fd={}", path, fd);
    fd
}

/// SYS_GETPID: return the calling process's ID.
///
/// Returns the ID of the currently-running task. For our single-process
/// system, that's `main` (id=0) — the user program runs in main's
/// task context (main jumped to user mode via `enter_user_mode`). Once
/// `fork` (4.3.b) creates real separate tasks, this returns meaningful
/// per-process IDs.
fn sys_getpid() -> u64 {
    let pid = crate::task::current_pid();
    println!("[syscall] sys_getpid -> {}", pid);
    pid.into()
}

/// SYS_EXIT: terminate the calling process with the given exit code.
///
/// Marks the current task as Zombie (so it won't be rescheduled), then
/// yields. yield_now will pick a different task (or halt if none are
/// runnable). We never come back from yield_now — if we did, it'd be a
/// bug (a Zombie got scheduled).
///
/// Limitations (improved in later slices):
///   - No resource cleanup (don't free the task's kernel stack, address
///     space, etc.).
///   - No parent notification (no `wait` syscall yet to read exit code).
///   - No reparenting of children to init (we don't have init).
fn sys_exit(exit_code: u64) -> u64 {
    println!("[syscall] sys_exit({})", exit_code);
    crate::task::mark_current_zombie(exit_code as i64);
    crate::task::yield_now();
    panic!("sys_exit: zombie task was rescheduled (this should never happen)");
}

/// SYS_WAIT4: wait for a child to exit, reap it, return its PID and
/// write its exit status to the user-provided pointer if non-NULL.
///
/// Linux semantics (simplified):
///   - If parent has a Zombie child: reap it immediately. Return child PID.
///   - If parent has children but none are Zombie: block (yield) until one
///     exits. Then reap.
///   - If parent has no children at all: return -1 (ECHILD).
///
/// Args:
///   - arg1 (rdi): target PID, or `u64::MAX` to wait for any child.
///   - arg2 (rsi): pointer to a user i64 where exit status will be written,
///     or 0 to ignore.
///
/// Returns:
///   - Child PID on success.
///   - `u64::MAX` (which user code reads as -1) if no children.
fn sys_wait(target_pid_raw: u64, exit_status_ptr: u64) -> u64 {
    use crate::process::Pid;
    use crate::task::{WaitResult, WaitTarget};

    let parent_pid = crate::task::current_pid();
    let target = if target_pid_raw == u64::MAX {
        WaitTarget::Any
    } else if target_pid_raw > 0 {
        WaitTarget::Specific(Pid::new(target_pid_raw))
    } else {
        println!(
            "[syscall] sys_wait4 unsupported target pid={} (parent pid={})",
            target_pid_raw, parent_pid
        );
        return u64::MAX;
    };

    println!(
        "[syscall] sys_wait4 (parent pid={}, target={:?})",
        parent_pid, target
    );

    loop {
        match crate::task::wait_for_zombie_child(parent_pid, target) {
            WaitResult::Reaped { child_pid, exit_status } => {
                println!(
                    "[syscall] sys_wait4 -> child pid={} (exit={})",
                    child_pid, exit_status
                );
                // Write exit status to user pointer if non-NULL.
                if exit_status_ptr != 0 {
                    // SAFETY: the user passed us this pointer; we trust it
                    // points to a writable i64 in user memory. Kernel can
                    // write to user-mapped pages (USER_ACCESSIBLE bit set
                    // in PT entry; kernel mode bypasses US check).
                    let user_ptr = exit_status_ptr as *mut i64;
                    unsafe {
                        core::ptr::write_volatile(user_ptr, exit_status);
                    }
                }
                return child_pid.into();
            }
            WaitResult::NoChildren => {
                println!("[syscall] sys_wait4 -> no matching children, returning -1");
                return u64::MAX; // -1 / ECHILD
            }
            WaitResult::WouldBlock => {
                println!("[syscall] sys_wait4 waiting for target {:?}", target);
                crate::task::yield_now();
                // When yield_now returns, the parent checks the target again.
            }
        }
    }
}

/// SYS_FORK: clone the calling process.
///
/// Returns:
///   - In the parent: the child's PID (a positive integer).
///   - In the child: 0.
///
/// What it does:
///   1. Create a new AddressSpace for the child (AddressSpace::new()
///      copies the parent's kernel-half PML4 entries 256–511; then
///      deep_clone_from_current() walks the user half 0–255, sharing
///      non-user frames via entry copy and copying user frames to
///      fresh frames for proper isolation).
///   2. For each user page (user code page + 16 user stack pages):
///      allocate a fresh frame, copy the parent's bytes into it, unmap
///      the parent's frame from the child's AS, then map the fresh frame
///      at the same virtual address. Now the child has its own private
///      copy of every user page.
///   3. Read the saved user state (RIP/RSP/RFLAGS + 6 callee-saved
///      registers) from the globals that `syscall_entry` populated.
///   4. Hand everything to `task::spawn_fork_child`, which:
///      - allocates a fresh kernel stack for the child
///      - populates it with the parent's callee-saved values +
///        fork_trampoline return address
///      - sets parent/children pointers
///      - adds the child to the scheduler as Ready
///   5. Return the child's PID. (The parent gets this value in RAX.
///      The child, when it eventually runs, will get 0 from
///      fork_trampoline's `xor rax, rax`.)
fn sys_fork() -> u64 {
    use crate::task::{spawn_fork_child, SavedCalleeSaves};

    println!("[syscall] sys_fork — cloning current process");

    // Disable interrupts for the entire fork operation. Fork touches the
    // SCHEDULER (via current_pid and spawn_fork_child) and walks page
    // tables; if the timer fired mid-fork, its handler would try to lock
    // SCHEDULER and could deadlock. Easier to just hold interrupts off
    // for the few microseconds fork takes.
    let was_enabled = x86_64::instructions::interrupts::are_enabled();
    if was_enabled {
        x86_64::instructions::interrupts::disable();
    }

    // Note: per-page cloning of user pages is no longer done here.
    // `spawn_fork_child` calls `AddressSpace::deep_clone_from_current()`,
    // which walks the parent's page-table tree and gives the child its
    // own fresh PDPT/PD/PT frames AND its own private copies of every
    // user page (allocate fresh frame + memcpy 4 KB). Kernel pages are
    // shared (their PT entries are copied as-is).

    // Step 3: Snapshot the saved user state from the globals populated by
    // syscall_entry. This is a SNAPSHOT — we read once and pass to
    // spawn_fork_child, which stores the values on the child's kernel
    // stack. After this point, the globals can be overwritten by future
    // syscalls without affecting the child.
    let saved_callee_saves = SavedCalleeSaves {
        rbx: USER_RBX_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        rbp: USER_RBP_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        r12: USER_R12_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        r13: USER_R13_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        r14: USER_R14_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        r15: USER_R15_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        user_rip: USER_RIP_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        user_rsp: USER_RSP_SAVE.load(core::sync::atomic::Ordering::SeqCst),
        user_rflags: USER_RFLAGS_SAVE.load(core::sync::atomic::Ordering::SeqCst),
    };

    // Step 4: Get parent PID, then spawn the child task.
    let parent_pid = crate::task::current_pid();
    let child_pid = spawn_fork_child(parent_pid, saved_callee_saves);

    println!(
        "[syscall] sys_fork -> child pid={} (parent pid={})",
        child_pid, parent_pid
    );

    // Re-enable interrupts if they were on when we entered.
    if was_enabled {
        x86_64::instructions::interrupts::enable();
    }

    // Step 5: Return child PID. Parent gets this in RAX.
    child_pid.into()
}

/// SYS_EXECVE: replace the calling process's program with a new one.
///
/// Args:
///   - arg1 (rdi): pointer to a null-terminated program name in user memory.
///
/// The name is looked up against `PROGRAMS` in `user.rs` (currently a tiny
/// static registry of two hand-assembled programs: "hello" and "goodbye").
///
/// On success: never returns. The old user code/stack are gone; control
/// transfers to the new program's entry point (start of the loaded bytes).
///
/// On failure (unknown program): panic. We don't have a syscall error
/// convention yet; can be refined later.
fn sys_exec(program_name_ptr: u64) -> u64 {
    use crate::user::{PROGRAMS, USER_CODE_ADDR, USER_RFLAGS_INIT, USER_STACK_BOTTOM, USER_STACK_TOP};
    use x86_64::structures::paging::PageTableFlags;

    println!("[syscall] sys_exec (name_ptr=0x{:x})", program_name_ptr);

    // Disable interrupts for the whole operation — touches SCHEDULER and
    // allocates frames; the timer firing mid-exec would deadlock on the lock.
    let was_enabled = x86_64::instructions::interrupts::are_enabled();
    if was_enabled {
        x86_64::instructions::interrupts::disable();
    }

    // Step 1: Read program name from user memory (null-terminated, max 32 bytes).
    let mut name_buf = [0u8; 32];
    let mut name_len = 0;
    for i in 0..32 {
        // SAFETY: user pointer from syscall arg. We trust it points to a
        // readable byte in user memory.
        let byte = unsafe {
            core::ptr::read_volatile((program_name_ptr + i as u64) as *const u8)
        };
        if byte == 0 {
            break;
        }
        name_buf[i] = byte;
        name_len = i + 1;
    }
    let name = core::str::from_utf8(&name_buf[..name_len])
        .expect("sys_exec: program name not utf-8");
    println!("[exec] looking up program {:?}", name);

    // Step 2: Look up program in PROGRAMS registry.
    let program_bytes: &[u8] = PROGRAMS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, b)| *b)
        .unwrap_or_else(|| panic!("sys_exec: program {:?} not in PROGRAMS registry", name));
    println!(
        "[exec] found program {:?} ({} bytes)",
        name,
        program_bytes.len()
    );

    // Step 3: Allocate fresh AddressSpace with INDEPENDENT page tables.
    // AddressSpace::new() copies kernel-half entries 256–511 (shares the
    // kernel subtree verbatim). deep_clone_from_current() then walks the
    // user half 0–255, allocating fresh PDPT/PD/PT frames so we can
    // safely unmap inherited user pages without affecting the current AS.
    let mut new_as = crate::memory::AddressSpace::new();
    new_as.deep_clone_from_current();

    // Step 3.5: Unmap the existing user code page and 16 user stack pages
    // from the new AS (inherited from the current AS via deep_clone). We're
    // about to replace them with fresh mappings for the new program.
    new_as
        .unmap(VirtAddr::new(USER_CODE_ADDR))
        .expect("sys_exec: unmap of inherited code page failed");
    let mut stack_virt = USER_STACK_BOTTOM;
    while stack_virt < USER_STACK_TOP {
        new_as
            .unmap(VirtAddr::new(stack_virt))
            .expect("sys_exec: unmap of inherited stack page failed");
        stack_virt += 4096;
    }

    // Step 4: Allocate fresh user code page, copy program bytes in.
    let code_frame = frame_allocator::allocate()
        .expect("sys_exec: out of frames for code page");
    let code_phys = PhysAddr::new(code_frame * 4096);
    crate::memory::write_to_frame(code_phys, program_bytes);
    // SAFETY: virtual address USER_CODE_ADDR was just unmapped above.
    unsafe {
        new_as
            .map_to(
                VirtAddr::new(USER_CODE_ADDR),
                code_phys,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE,
            )
            .expect("sys_exec: map_to code failed");
    }

    // Step 5: Allocate fresh user stack pages (16 pages = 64 KB).
    let mut stack_virt = USER_STACK_BOTTOM;
    while stack_virt < USER_STACK_TOP {
        let frame = frame_allocator::allocate()
            .expect("sys_exec: out of frames for stack page");
        let phys = PhysAddr::new(frame * 4096);
        // SAFETY: each page in [USER_STACK_BOTTOM, USER_STACK_TOP) was just
        // unmapped above.
        unsafe {
            new_as
                .map_to(
                    VirtAddr::new(stack_virt),
                    phys,
                    PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::USER_ACCESSIBLE,
                )
                .expect("sys_exec: map_to stack failed");
        }
        stack_virt += 4096;
    }

    // Step 6: Update the current task's address_space (and name, for clarity).
    // The task struct stays — same PID, same kernel stack, same scheduler entry.
    crate::task::replace_current_address_space(new_as, name);

    // Step 7: Switch CR3 to the new AS. After this point, the old user
    // mappings are gone — if we accessed them now we'd page fault. But we
    // don't need to; everything from here on is kernel-side.
    let pml4_frame = crate::task::current_pml4_frame();
    // SAFETY: the frame is owned by the current task's AddressSpace and was
    // properly initialized in AddressSpace::new() + map_to calls above.
    unsafe {
        x86_64::registers::control::Cr3::write(
            pml4_frame,
            x86_64::registers::control::Cr3Flags::empty(),
        );
    }

    // Step 8: Jump to user mode at the new program's entry. This is the
    // "exec trampoline" — same idea as fork_trampoline but with values
    // coming from the new program (entry = USER_CODE_ADDR, RSP = top of
    // fresh user stack, RFLAGS = IF + reserved bit).
    //
    // Done as inline asm with explicit register constraints:
    //   - in("rcx") user_rip   — sysretq reads RIP from RCX
    //   - in("r11") user_rflags — sysretq reads RFLAGS from R11
    //   - in(reg)  user_rsp    — we mov it into RSP just before sysretq
    //
    // The "xor rax, rax" happens AFTER the rsp load so any reg the compiler
    // chose for user_rsp is no longer needed by the time we clobber RAX.
    let user_rip = USER_CODE_ADDR;
    let user_rsp = USER_STACK_TOP;
    let user_rflags = USER_RFLAGS_INIT;

    println!(
        "[exec] jumping to user mode: rip=0x{:x} rsp=0x{:x} flags=0x{:x}",
        user_rip, user_rsp, user_rflags,
    );

    // SAFETY: this never returns. sysretq transfers control to user mode
    // at USER_CODE_ADDR. We're in cli state (disabled above), and the user
    // RFLAGS has IF=1, so sysretq will flip interrupts back on.
    unsafe {
        core::arch::asm!(
            "mov rsp, {rsp}",                // switch to user stack
            "xor rax, rax",                  // RAX = 0 (convention for exec)
            "cli",                           // (defensive) ensure IF=0 for sysretq
            "sysretq",
            in("rcx") user_rip,
            in("r11") user_rflags,
            rsp = in(reg) user_rsp,
            options(noreturn),
        );
    }
}
