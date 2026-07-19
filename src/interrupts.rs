//! Interrupt Descriptor Table (IDT) and exception handlers.

use crate::println;
use core::arch::naked_asm;
use core::sync::atomic::{AtomicUsize, Ordering};
use lazy_static::lazy_static;
use x86_64::structures::idt::{
    HandlerFunc, InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode,
    PageFaultHandlerFunc,
};

/// Total number of timer ticks since boot. The handler increments this
/// every tick. Public so other modules (e.g. main's monitoring loop)
/// can read it for periodic actions.
pub static TICKS: AtomicUsize = AtomicUsize::new(0);
pub static VIRTIO_BLK_INTERRUPTS: AtomicUsize = AtomicUsize::new(0);
pub static VIRTIO_NET_RX_INTERRUPTS: AtomicUsize = AtomicUsize::new(0);
pub static VIRTIO_NET_TX_INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

/// Vector number for the timer (PIC IRQ 0, mapped to vector 0x20 = 32).
/// Matches `crate::pic::PIC_1_OFFSET`.
const TIMER_VECTOR: u8 = 0x20;
pub const VIRTIO_BLK_VECTOR: u8 = 0x21;
pub const VIRTIO_NET_RX_VECTOR: u8 = 0x30;
pub const VIRTIO_NET_TX_VECTOR: u8 = 0x31;

lazy_static! {
    /// The kernel's IDT. Add handlers here as we implement them.
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        // Cast each naked wrapper to the typed HandlerFunc. The CPU only
        // stores the function address in the IDT — Rust's ABI distinctions
        // don't matter at the hardware level. Our wrappers are naked (no
        // Rust-generated prologue), so there's no actual ABI mismatch.
        let bp: HandlerFunc =
            unsafe { core::mem::transmute(breakpoint_wrapper as unsafe extern "C" fn()) };
        idt.breakpoint.set_handler_fn(bp);

        let timer: HandlerFunc =
            unsafe { core::mem::transmute(timer_wrapper as unsafe extern "C" fn()) };
        idt[TIMER_VECTOR].set_handler_fn(timer);

        let virtio_blk: HandlerFunc =
            unsafe { core::mem::transmute(virtio_blk_wrapper as unsafe extern "C" fn()) };
        idt[VIRTIO_BLK_VECTOR].set_handler_fn(virtio_blk);

        let virtio_net_rx: HandlerFunc =
            unsafe { core::mem::transmute(virtio_net_rx_wrapper as unsafe extern "C" fn()) };
        idt[VIRTIO_NET_RX_VECTOR].set_handler_fn(virtio_net_rx);

        let virtio_net_tx: HandlerFunc =
            unsafe { core::mem::transmute(virtio_net_tx_wrapper as unsafe extern "C" fn()) };
        idt[VIRTIO_NET_TX_VECTOR].set_handler_fn(virtio_net_tx);

        // Page fault handler (vector 14). Different signature: has an
        // error code pushed by the CPU. The wrapper extracts both the
        // stack frame and the error code and passes them to the inner.
        // The transmute target type must match what set_handler_fn
        // expects: specifically PageFaultHandlerFunc (which is
        // extern "x86-interrupt" fn(&InterruptStackFrame, PageFaultErrorCode)).
        let pf: PageFaultHandlerFunc =
            unsafe { core::mem::transmute(page_fault_wrapper as unsafe extern "C" fn()) };
        idt.page_fault.set_handler_fn(pf);

        idt
    };
}

#[unsafe(naked)]
unsafe extern "C" fn virtio_blk_wrapper() {
    naked_asm!(
        "push rax", "push rbx", "push rcx", "push rdx", "push rsi",
        "push rdi", "push rbp", "push r8", "push r9", "push r10",
        "push r11", "push r12", "push r13", "push r14", "push r15",
        "call {inner}",
        "pop r15", "pop r14", "pop r13", "pop r12", "pop r11",
        "pop r10", "pop r9", "pop r8", "pop rbp", "pop rdi",
        "pop rsi", "pop rdx", "pop rcx", "pop rbx", "pop rax",
        "iretq",
        inner = sym virtio_blk_inner,
    );
}

extern "C" fn virtio_blk_inner() {
    VIRTIO_BLK_INTERRUPTS.fetch_add(1, Ordering::Relaxed);
    crate::apic::end_of_interrupt();
}

/// Naked wrapper for the virtio-net RX interrupt (vector 0x30).
///
/// Same shape as `virtio_blk_wrapper`: save all 15 general-purpose
/// registers, call the inner handler, restore, `iretq`. The 40-byte CPU
/// frame plus 120 bytes of pushes leaves RSP 16-aligned for the call.
#[unsafe(naked)]
unsafe extern "C" fn virtio_net_rx_wrapper() {
    naked_asm!(
        "push rax", "push rbx", "push rcx", "push rdx", "push rsi",
        "push rdi", "push rbp", "push r8", "push r9", "push r10",
        "push r11", "push r12", "push r13", "push r14", "push r15",
        "call {inner}",
        "pop r15", "pop r14", "pop r13", "pop r12", "pop r11",
        "pop r10", "pop r9", "pop r8", "pop rbp", "pop rdi",
        "pop rsi", "pop rdx", "pop rcx", "pop rbx", "pop rax",
        "iretq",
        inner = sym virtio_net_rx_inner,
    );
}

/// The RX interrupt does almost nothing itself: count it, wake the
/// deferred network task, EOI. The actual work (draining the RX ring,
/// parsing frames, delivering datagrams, replying to ARP/ICMP) happens
/// in `network_task` — task context, where printing, locking, and
/// transmitting are all safe. This split is Linux's NAPI shape: the
/// hardirq handler schedules the poll; the poll runs in a context that
/// can sleep.
///
/// Why not dispatch here: `poll_rx` prints to the serial port and can
/// transmit replies (ARP reply, ICMP echo reply). Printing takes the
/// serial spinlock — if this ISR interrupted a task holding that lock,
/// locking again would deadlock. Transmitting sleeps in `send_frame`'s
/// hlt wait, which an ISR must not do. Both rules were learned the hard
/// way (see the timer ISR's no-printing rule from Lesson 28).
extern "C" fn virtio_net_rx_inner() {
    VIRTIO_NET_RX_INTERRUPTS.fetch_add(1, Ordering::Relaxed);
    if let Some(pid) = crate::virtio::net_task_pid() {
        crate::task::wake(pid);
    }
    crate::apic::end_of_interrupt();
}

/// Naked wrapper for the virtio-net TX interrupt (vector 0x31). The
/// handler only counts and EOIs — `send_frame` re-checks the TX used
/// ring itself after its hlt wakes, the same design as virtio-blk.
#[unsafe(naked)]
unsafe extern "C" fn virtio_net_tx_wrapper() {
    naked_asm!(
        "push rax", "push rbx", "push rcx", "push rdx", "push rsi",
        "push rdi", "push rbp", "push r8", "push r9", "push r10",
        "push r11", "push r12", "push r13", "push r14", "push r15",
        "call {inner}",
        "pop r15", "pop r14", "pop r13", "pop r12", "pop r11",
        "pop r10", "pop r9", "pop r8", "pop rbp", "pop rdi",
        "pop rsi", "pop rdx", "pop rcx", "pop rbx", "pop rax",
        "iretq",
        inner = sym virtio_net_tx_inner,
    );
}

extern "C" fn virtio_net_tx_inner() {
    VIRTIO_NET_TX_INTERRUPTS.fetch_add(1, Ordering::Relaxed);
    crate::apic::end_of_interrupt();
}

/// Load our IDT. After this, the CPU consults our table when an
/// exception or interrupt fires.
pub fn init() {
    IDT.load();
}

/// Naked wrapper around the breakpoint handler.
///
/// The CPU enters with RSP that is **8-mod-16 misaligned** — the 40-byte
/// interrupt frame it pushes (RIP, CS, RFLAGS, RSP, SS) is not a multiple
/// of 16. Rust's `extern "x86-interrupt"` ABI assumes RSP is 16-aligned,
/// so its compiler-generated prologue uses `movaps` to save XMM registers,
/// which faults on misaligned stacks.
///
/// This wrapper has no Rust prologue (`#[naked]`), so we can manually
/// realign the stack before calling normal Rust code that may use XMM.
///
/// Stack layout on entry (RSP points at the CPU-pushed frame):
///   [rsp]      RIP        ← return address
///   [rsp+8]    CS
///   [rsp+16]   RFLAGS
///   [rsp+24]   RSP_old
///   [rsp+32]   SS_old
///
/// TODO: This wrapper does not save/restore caller-saved registers
/// (rax, rcx, rdx, rsi, rdi, r8–r11). The interrupted code will see
/// those change. Fine for `int3` (interrupted code doesn't care), but
/// needs fixing before production handlers like page fault.
#[unsafe(naked)]
unsafe extern "C" fn breakpoint_wrapper() {
    naked_asm!(
        "sub rsp, 8",                // Realign RSP to 16 bytes
        "lea rdi, [rsp + 8]",        // rdi = &InterruptStackFrame (first arg)
        "call {inner}",              // Call normal Rust function
        "add rsp, 8",                // Undo realignment
        "iretq",                     // Return from interrupt
        inner = sym breakpoint_inner,
    );
}

/// The actual breakpoint handler. Standard Rust function — can use
/// any registers, format strings, println!, etc., because the wrapper
/// guaranteed 16-byte stack alignment on entry.
extern "C" fn breakpoint_inner(stack_frame: &InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

/// Naked wrapper for the timer interrupt (vector 0x20 / PIC IRQ 0).
///
/// Unlike `breakpoint_wrapper`, this one saves *all* general-purpose
/// registers (not just realigns the stack). This is critical because
/// the handler calls `task::yield_now()` which switches to another task.
/// The interrupted code could be using any register at the moment of
/// preemption — if we only saved callee-saved registers (as `switch`
/// does), the interrupted code would see its caller-saved registers
/// (RAX, RCX, RDX, RSI, RDI, R8-R11) clobbered when it eventually
/// resumes.
///
/// Stack layout on entry (the CPU has already pushed the 40-byte
/// interrupt frame): RIP, CS, RFLAGS, RSP, SS.
///
/// After our 15 pushes (120 bytes), total offset from the original
/// pre-interrupt RSP is 40 + 120 = 160 bytes, which is a multiple of
/// 16 — so RSP ends up 16-aligned for the `call`.
#[unsafe(naked)]
unsafe extern "C" fn timer_wrapper() {
    naked_asm!(
        // Push all 15 general-purpose registers (in any order, as long
        // as we pop in the reverse order below).
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // RSP is now 16-aligned (40-byte CPU frame + 120 bytes of our pushes).
        "call {inner}",
        // Restore all 15 registers in reverse order.
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        // iretq pops the 5-word interrupt frame the CPU pushed.
        "iretq",
        inner = sym timer_inner,
    );
}

/// The actual timer handler. Increments the tick counter, acknowledges
/// both interrupt controllers, then preempts the current task
/// by calling `yield_now()`. The EOI must come *before* the yield —
/// otherwise the next tick stays queued in the PIC and we never
/// preempt again.
///
/// Interrupt handlers must not print: a timer can interrupt code holding
/// the serial lock, and trying to acquire it again would deadlock.
/// Because the wrapper saved all GP registers, this function (and
/// anything it calls, including `yield_now` → `switch`) can freely
/// clobber any register. The wrapper restores them on the way out.
extern "C" fn timer_inner() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    // Bounded-latency backstop for deferred RX while virtio-net MSI-X
    // delivery remains unreliable in the current QEMU configuration.
    if let Some(pid) = crate::virtio::net_task_pid() {
        crate::task::wake(pid);
    }
    // Send EOI before preempting so the next tick can fire.
    unsafe {
        crate::pic::PICS
            .lock()
            .notify_end_of_interrupt(TIMER_VECTOR);
    }
    // The legacy PIC reaches the CPU through the Local APIC's ExtINT
    // virtual-wire input once xAPIC is enabled. Both controllers need EOI.
    crate::apic::end_of_interrupt();
    // Preempt: switch to the next task. When we eventually get switched
    // back to, we resume here, then iretq returns to wherever the
    // interrupted task was.
    crate::task::yield_now();
}

/// Naked wrapper for the page fault handler (vector 14).
///
/// Unlike breakpoint/timer, page fault pushes an error code onto the
/// stack before the interrupt frame. The wrapper must read both the
/// error code (at [rsp]) and pass a pointer to the interrupt frame
/// ([rsp+8] onwards).
///
/// Stack on entry (RSP points here):
///   [rsp]      error_code   ← pushed by CPU
///   [rsp+8]    RIP          ← start of InterruptStackFrame
///   [rsp+16]   CS
///   [rsp+24]   RFLAGS
///   [rsp+32]   RSP_old
///   [rsp+40]   SS_old
#[unsafe(naked)]
unsafe extern "C" fn page_fault_wrapper() {
    naked_asm!(
        "sub rsp, 8",                // Realign to 16 bytes
        "lea rdi, [rsp + 16]",       // rdi = &InterruptStackFrame (skip error_code + padding)
        "mov rsi, [rsp + 8]",        // rsi = error_code
        "call {inner}",
        "add rsp, 8",                // Undo realignment
        "add rsp, 8",                // Skip error_code (CPU pushed it; iretq doesn't pop it)
        "iretq",
        inner = sym page_fault_inner,
    );
}

/// The actual page fault handler. Prints the faulting address (from CR2),
/// decodes the error code bits, and shows what the user program wrote to
/// its own memory before faulting. Then halts — we don't yet have a way
/// to cleanly kill a user program (that's M4 fork/wait territory).
extern "C" fn page_fault_inner(stack_frame: &InterruptStackFrame, error_code: u64) {
    use x86_64::registers::control::Cr2;

    let cr2 = Cr2::read();

    println!("\nEXCEPTION: PAGE FAULT");
    println!("  tried to access: {:?}", cr2);
    println!(
        "  error code: {:#b}  (P={}, W/R={}, U/S={}, RSVD={})",
        error_code,
        error_code & 1,        // bit 0: page present?
        (error_code >> 1) & 1, // bit 1: write vs read
        (error_code >> 2) & 1, // bit 2: user vs supervisor
        (error_code >> 3) & 1, // bit 3: reserved bits violation
    );

    // Read what the user wrote to its own memory (kernel can access any
    // mapped memory). The user program writes 0xDEADBEEF to 0x7000_0000
    // before triggering the fault — proves user code actually ran.
    let user_magic_addr = 0x7000_0000usize as *const u64;
    let user_magic = unsafe { core::ptr::read_volatile(user_magic_addr) };
    println!(
        "  user wrote to its memory at 0x7000_0000: 0x{:x}",
        user_magic
    );

    println!("\n{:#?}", stack_frame);

    println!("\n[page_fault] halting — no clean way to kill user program yet");
    loop {
        x86_64::instructions::hlt();
    }
}
