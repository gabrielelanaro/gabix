//! Minimal test runner for kernel-mode tests.
//!
//! Pattern from phil-opp's "Testing" post. A test is any function that
//! takes no arguments and returns `()`. Tests are collected into a slice
//! and run in sequence. If any test panics, control jumps straight to
//! the panic handler (which in test mode calls `exit_qemu(EXIT_FAILURE)`,
//! so the wrapper script sees a failure exit code). If every test runs
//! to completion without panicking, this module calls
//! `exit_qemu(EXIT_SUCCESS)` and the wrapper script sees a pass.
//!
//! Usage from `main.rs`:
//!
//! ```ignore
//! test_runner::run_tests(&[
//!     &memory::address_space_roundtrip,
//!     &elf::magic_correct,
//!     // ...
//! ]);
//! ```
//!
//! Each boot runs every test in the list. A panic in test N ends the
//! whole run — tests N+1, N+2, ... do not get to report. Accept that
//! tradeoff for now in exchange for the simple "panic = end of run"
//! invariant. If a test needs to mutate global state in a way that
//! could break later tests, give it its own boot via a separate feature
//! flag instead of stacking it in here.

use crate::print;
use crate::println;
use crate::qemu_exit;

/// A thing that can be run as a test.
///
/// Blanket-implemented for every `Fn()` so test functions can be passed
/// by reference (`&foo`) without explicit wrapping.
pub trait TestCase {
    fn run(&self);
}

impl<T> TestCase for T
where
    T: Fn(),
{
    fn run(&self) {
        self();
    }
}

/// Run every test in `tests`, then terminate QEMU with success.
///
/// A panic inside any test jumps straight to the panic handler, which
/// in test mode calls `exit_qemu(EXIT_FAILURE)`. So if control reaches
/// the bottom of this function, every test ran without panicking, and
/// we exit with success.
pub fn run_tests(tests: &[&dyn TestCase]) {
    println!("[test-runner] running {} tests", tests.len());
    for (i, test) in tests.iter().enumerate() {
        print!("[test-runner] test {}/{} ... ", i + 1, tests.len());
        test.run();
        println!("ok");
    }
    println!(
        "[test-runner] all {} tests passed — exiting QEMU",
        tests.len()
    );
    qemu_exit::exit_qemu(qemu_exit::EXIT_SUCCESS);
}
