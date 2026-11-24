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
/// Matches `super::pic::PIC_1_OFFSET`.
const TIMER_VECTOR: u8 = super::pic::PIC_1_OFFSET;
// MSI-X vectors programmed by the block and network drivers. PIC IRQ 1
// shares vector 0x21 but remains masked in pic::init().
pub const VIRTIO_BLK_VECTOR: u8 = 0x21;
pub const VIRTIO_NET_RX_VECTOR: u8 = 0x30;
pub const VIRTIO_NET_TX_VECTOR: u8 = 0x31;

// Intel SDM Vol. 3A, section 6.14.2 (64-Bit Mode Stack Frame):
// interrupt stack slots are eight bytes in 64-bit mode. The CPU aligns
// RSP to 16 bytes, then pushes SS, RSP, RFLAGS, CS, RIP (40 bytes).
const INTERRUPT_STACK_WORD_BYTES: usize = core::mem::size_of::<u64>();
const BREAKPOINT_CALL_PADDING_BYTES: usize = INTERRUPT_STACK_WORD_BYTES;
// Retained from the existing page-fault wrapper. TODO: its error code already
// brings the CPU frame to 48 bytes; this extra padding misaligns the C call.
const PAGE_FAULT_CALL_PADDING_BYTES: usize = INTERRUPT_STACK_WORD_BYTES;
const PAGE_FAULT_FRAME_OFFSET: usize = PAGE_FAULT_CALL_PADDING_BYTES + INTERRUPT_STACK_WORD_BYTES;

// Address used by the original user-mode fault demonstration. The fatal
// handler still reads it unconditionally; it may itself fault if unmapped.
const FAULT_DEMO_USER_ADDRESS: usize = 0x7000_0000;

lazy_static! {
    /// The kernel's IDT. Add handlers here as we implement them.
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        // IDT entries hold wrapper addresses. Each naked wrapper must
        // implement the CPU entry/return convention and prepare the C call
        // itself; this transmute supplies the type required by set_handler_fn.
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
        // extern "x86-interrupt" fn(InterruptStackFrame, PageFaultErrorCode)).
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
    super::apic::end_of_interrupt();
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

/// Count the receive interrupt, wake `net::device::network_task`, and
/// acknowledge the local APIC. The task drains the receive ring and handles
/// protocols after the scheduler runs it. Sending replies can wait for TX
/// completion, so receive processing runs in task context.
extern "C" fn virtio_net_rx_inner() {
    VIRTIO_NET_RX_INTERRUPTS.fetch_add(1, Ordering::Relaxed);
    if let Some(pid) = crate::net::device::net_task_pid() {
        crate::task::wake(pid);
    }
    super::apic::end_of_interrupt();
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
    super::apic::end_of_interrupt();
}

/// Load our IDT. After this, the CPU consults our table when an
/// exception or interrupt fires.
pub fn init() {
    IDT.load();
}

/// Breakpoint entry has a five-word CPU frame: RIP, CS, RFLAGS, old RSP,
/// old SS, starting at [rsp]. Its 40 bytes leave RSP eight bytes short of
/// the 16-byte alignment required before a normal C call.
///
/// TODO: this wrapper preserves no caller-saved general registers or SIMD
/// state. The Rust call can corrupt the interrupted computation, including
/// code that deliberately executes int3.
#[unsafe(naked)]
unsafe extern "C" fn breakpoint_wrapper() {
    naked_asm!(
        "sub rsp, {padding}",        // Align RSP before the C call
        "lea rdi, [rsp + {padding}]", // First C argument: CPU frame
        "call {inner}",              // Call normal Rust function
        "add rsp, {padding}",        // Restore RSP to the CPU frame
        "iretq",                     // Return from interrupt
        inner = sym breakpoint_inner,
        padding = const BREAKPOINT_CALL_PADDING_BYTES,
    );
}

/// Print the saved CPU frame. The wrapper aligns the call stack; its
/// missing register preservation remains a separate correctness issue.
extern "C" fn breakpoint_inner(stack_frame: &InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

/// Naked wrapper for the timer interrupt (vector 0x20 / PIC IRQ 0).
///
/// Save the 15 general-purpose registers before calling the scheduler.
/// A task switch can suspend this handler; when that task runs again, the
/// wrapper restores its registers and IRETQ restores its CPU frame.
///
/// The CPU aligns the incoming stack before pushing its 40-byte frame.
/// Our 120 bytes of register saves make RSP 16-byte aligned for CALL.
/// SIMD/floating-point state is not saved by these wrappers.
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
/// The wrapper restores general-purpose registers when the task resumes.
/// SIMD/floating-point preservation requires separate support.
extern "C" fn timer_inner() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    // Bounded-latency backstop for deferred RX while virtio-net MSI-X
    // delivery remains unreliable in the current QEMU configuration.
    if let Some(pid) = crate::net::device::net_task_pid() {
        crate::task::wake(pid);
    }
    // Send EOI before preempting so the next tick can fire.
    unsafe {
        super::pic::PICS
            .lock()
            .notify_end_of_interrupt(TIMER_VECTOR);
    }
    // The legacy PIC reaches the CPU through the Local APIC's ExtINT
    // virtual-wire input once xAPIC is enabled. Both controllers need EOI.
    super::apic::end_of_interrupt();
    // Preempt: switch to the next task. When we eventually get switched
    // back to, we resume here, then iretq returns to wherever the
    // interrupted task was.
    crate::task::yield_now();
}

/// Naked wrapper for the page fault handler (vector 14).
///
/// The CPU pushes an error code after the five-word interrupt frame,
/// leaving the code at [rsp] and the frame at [rsp+8]. The inner handler
/// receives the frame pointer in RDI and the error code in RSI.
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
        "sub rsp, {padding}",        // Existing padding; see alignment TODO above
        "lea rdi, [rsp + {frame_offset}]", // Skip padding and error code
        "mov rsi, [rsp + {padding}]", // Second C argument: error code
        "call {inner}",
        "add rsp, {padding}",        // Remove wrapper padding
        "add rsp, {error_bytes}",    // IRETQ expects the frame without the error code
        "iretq",
        inner = sym page_fault_inner,
        padding = const PAGE_FAULT_CALL_PADDING_BYTES,
        frame_offset = const PAGE_FAULT_FRAME_OFFSET,
        error_bytes = const INTERRUPT_STACK_WORD_BYTES,
    );
}

/// Fatal diagnostic: print CR2, selected error bits, the demonstration's
/// user-memory value and the CPU frame, then halt. Process termination is
/// not wired into this handler. The demonstration read can trigger another
/// fault, and the wrapper's C-call alignment remains incorrect.
extern "C" fn page_fault_inner(stack_frame: &InterruptStackFrame, error_code: u64) {
    use x86_64::registers::control::Cr2;

    let cr2 = Cr2::read();
    let fault = PageFaultErrorCode::from_bits_truncate(error_code);

    println!("\nEXCEPTION: PAGE FAULT");
    println!("  tried to access: {:?}", cr2);
    println!(
        "  error code: {:#b}  (P={}, W/R={}, U/S={}, RSVD={})",
        error_code,
        u8::from(fault.contains(PageFaultErrorCode::PROTECTION_VIOLATION)),
        u8::from(fault.contains(PageFaultErrorCode::CAUSED_BY_WRITE)),
        u8::from(fault.contains(PageFaultErrorCode::USER_MODE)),
        u8::from(fault.contains(PageFaultErrorCode::MALFORMED_TABLE)),
    );

    // Original demonstration read. Arbitrary programs need not map this
    // address, so this diagnostic cannot be used as a general fault probe.
    let user_magic_addr = FAULT_DEMO_USER_ADDRESS as *const u64;
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
