//! Time-related syscall handlers.

use super::user::is_low_half_range;

#[repr(C)]
struct Timespec {
    seconds: i64,
    nanoseconds: i64,
}

pub(super) fn clock_gettime(clock_id: u64, timespec_ptr: u64) -> u64 {
    const CLOCK_MONOTONIC: u64 = 1;
    // Must match the PIT rate selected by arch::x86_64::boot.
    const TICKS_PER_SECOND: usize = 100;
    const NANOSECONDS_PER_SECOND: usize = 1_000_000_000;
    const NANOSECONDS_PER_TICK: usize = NANOSECONDS_PER_SECOND / TICKS_PER_SECOND;

    if clock_id != CLOCK_MONOTONIC
        || !is_low_half_range(timespec_ptr, core::mem::size_of::<Timespec>() as u64)
    {
        return u64::MAX;
    }

    let ticks = crate::arch::timer_ticks();
    let value = Timespec {
        seconds: (ticks / TICKS_PER_SECOND) as i64,
        nanoseconds: ((ticks % TICKS_PER_SECOND) * NANOSECONDS_PER_TICK) as i64,
    };
    // The caller must supply aligned, writable memory. The range check above
    // does not verify either requirement.
    unsafe {
        core::ptr::write(timespec_ptr as *mut Timespec, value);
    }
    0
}
