# User-pointer validation: range check is cheap, fault tolerance isn't

During Lesson 2 step 2.4, user pushed to add user-pointer validation
rather than defer it as "we'll get there." This was the right call.

**What we shipped (~30 lines, ~85 seconds):**
- `USER_ADDR_LIMIT = 0x0000_8000_0000_0000` (2^47, end of canonical lower half)
- `is_user_range(ptr, len) -> bool` helper that checks `ptr < limit` and `ptr.checked_add(len) <= limit`
- Used by `sys_read` and `sys_write` to reject kernel-pointer attacks (e.g., user passes `0xFFFFFFFF80000000` to read kernel memory)
- Returns `u64::MAX` (EFAULT sentinel) on bad pointer

**What we explicitly deferred:**
- Fault-tolerant copy (catching page faults during memcpy) — requires a page-fault handler we don't have. Without it, a pointer in user range but unmapped still triple-faults the kernel.
- Honest timeline: lands when page-fault handler lands (M5+), or never if security stays off-mission.

**Calibration note:** my original estimate for "user-pointer validation" was 30 minutes. Actual was 85 seconds. The conflation was "real Linux's copy_to_user (with exception tables)" vs "range check only." User correctly pushed to do the range check immediately given it's small.

**Implications for future sessions:**
- When user pushes back on a deferred item with "is it really that long?", separate the design thinking from the code. If the design is settled, code is usually minutes.
- The pattern of "small now > retrofit later" is now established for security/safety items
- All future syscalls taking user pointers should call `is_user_range()` first
