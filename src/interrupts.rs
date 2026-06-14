//! Interrupt Descriptor Table (IDT) and exception handlers.

use crate::println;
use core::arch::naked_asm;
use lazy_static::lazy_static;
use x86_64::structures::idt::{HandlerFunc, InterruptDescriptorTable, InterruptStackFrame};

/// Vector number for the timer (PIC IRQ 0, mapped to vector 0x20 = 32).
/// Matches `crate::pic::PIC_1_OFFSET`.
const TIMER_VECTOR: u8 = 0x20;

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

        idt
    };
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
/// Same pattern as breakpoint_wrapper: realign stack, call inner, return.
#[unsafe(naked)]
unsafe extern "C" fn timer_wrapper() {
    naked_asm!(
        "sub rsp, 8",
        "lea rdi, [rsp + 8]",
        "call {inner}",
        "add rsp, 8",
        "iretq",
        inner = sym timer_inner,
    );
}

/// The actual timer handler. Prints "tick" and sends EOI to the PIC so
/// the next tick can fire. Without the EOI, the PIC would never deliver
/// another timer interrupt.
extern "C" fn timer_inner(_stack_frame: &InterruptStackFrame) {
    println!("tick");
    unsafe {
        crate::pic::PICS
            .lock()
            .notify_end_of_interrupt(TIMER_VECTOR);
    }
}
