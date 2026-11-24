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
//! Wrappers recognize the specific expected status codes. An odd exit status
//! alone cannot distinguish this device from every host-side failure.
//! Reference: QEMU hw/misc/debugexit.c::debug_exit_write.

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
/// Requires the `isa-debug-exit` device attached at port `0xf4` for the intended
/// shutdown behavior. If execution continues after the port write, this function
/// panics. The effect of that port without the device depends on the machine;
/// this helper does not discover or validate the attached device.
pub fn exit_qemu(exit_code: u8) -> ! {
    let mut port: Port<u8> = Port::new(EXIT_PORT);
    unsafe {
        port.write(exit_code);
    }
    unreachable!("isa-debug-exit should have terminated QEMU");
}
