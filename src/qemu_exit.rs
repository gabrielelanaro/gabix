//! Terminate the QEMU process from inside the guest.
//!
//! QEMU target adapter for reporting kernel shutdown and panic status to the
//! host through `isa-debug-exit`.
//!
//! Requires the `isa-debug-exit` device on the QEMU command line:
//!   `-device isa-debug-exit,iobase=0xf4,iosize=0x04`
//!
//! QEMU's exit-code formula is `(byte << 1) | 1`, so the process always
//! exits with a non-zero code:
//!   - `EXIT_SUCCESS` (0x10) → QEMU exits 33 → wrapper script returns 0
//!   - `EXIT_FAILURE` (0x11) → QEMU exits 35 → wrapper script returns 1
//!
//! The LSB=1 invariant lets callers distinguish "guest exited via
//! isa-debug-exit" from "QEMU itself crashed" (which would be exit 0).

use x86_64::instructions::port::Port;

/// I/O port the isa-debug-exit device listens on. Matches the `iobase`
/// argument in the QEMU invocation.
const EXIT_PORT: u16 = 0xf4;

/// Success byte. Caller convention: write this after a clean init exit.
/// QEMU translates this to process exit code `(0x10 << 1) | 1 = 33`.
pub const EXIT_SUCCESS: u8 = 0x10;

/// Failure byte. Caller convention: write this when something failed.
/// QEMU translates this to process exit code `(0x11 << 1) | 1 = 35`.
pub const EXIT_FAILURE: u8 = 0x11;

/// Terminate the QEMU process with the given exit byte.
///
/// # Safety
/// Requires the `isa-debug-exit` device attached at port `0xf4`. If the
/// device isn't attached, the `out` instruction is a no-op and execution
/// continues into `unreachable!()`, which panics — and the panic handler
/// tries to call this function again, producing an infinite panic loop.
pub fn exit_qemu(exit_code: u8) -> ! {
    let mut port: Port<u8> = Port::new(EXIT_PORT);
    unsafe {
        port.write(exit_code);
    }
    unreachable!("isa-debug-exit should have terminated QEMU");
}
