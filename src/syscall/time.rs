//! Time-related syscall handlers.

use super::user::is_user_range;

#[repr(C)]
struct Timespec {
    seconds: i64,
    nanoseconds: i64,
}

pub(super) fn clock_gettime(clock_id: u64, timespec_ptr: u64) -> u64 {
    const CLOCK_MONOTONIC: u64 = 1;
    const TICKS_PER_SECOND: usize = 100;

    if clock_id != CLOCK_MONOTONIC
        || !is_user_range(timespec_ptr, core::mem::size_of::<Timespec>() as u64)
    {
        return u64::MAX;
    }

    let ticks = crate::arch::timer_ticks();
    let value = Timespec {
        seconds: (ticks / TICKS_PER_SECOND) as i64,
        nanoseconds: ((ticks % TICKS_PER_SECOND) * 10_000_000) as i64,
    };
    unsafe {
        core::ptr::write(timespec_ptr as *mut Timespec, value);
    }
    0
}
