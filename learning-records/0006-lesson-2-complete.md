# Lesson 2 complete — VFS foundation end-to-end

User completed Lesson 2 (all six sub-steps 2.1 through 2.6) in a single
session. End-to-end test passes: user program calls `open` → `read` →
`write` (to stdout) → `close` → `exit`, and "hello vfs" prints to serial
from the hardcoded inode's bytes.

**What was built:**
- `src/fs.rs` (new): `Inode`, `File`, `FdTable` types
- Per-process `fd_table: FdTable` field on `Task`
- `with_current_fd_table` accessor (closure form, irq-safe via `without_interrupts`)
- Four syscalls: `SYS_OPEN=2`, `SYS_READ=0`, `SYS_WRITE=1` (extended to dispatch on fd), `SYS_CLOSE=3`
- `USER_ADDR_LIMIT` + `is_user_range()` for user-pointer validation
- USER_PROGRAM rewritten as single-task VFS test
- HELLO_PROGRAM and GOODBYE_PROGRAM updated for Linux `sys_write(fd, buf, len)` convention
- Latent `yield_now` bug fixed (single-task zombie case bypassed halt check)

Committed as `72c215f` (single slice — user noted preference for per-step commits in future lessons, see [[feedback_commit_per_step]]).

**Zone of proximal development now:**
- Inode/File/FdTable types are concrete and exercised — user has earned these as baseline vocabulary
- User-pointer validation pattern is established — future syscalls taking pointers should follow it
- The "Vec vs Box vs Frame" allocator discussion landed — user understands the heap is one consumer of frames, not a separate pool
- Ready for Lesson 3: real inodes with path-indexed lookup, multiple files, replacing `HARDCODED_INODE`

**Calibration data point:** Lesson 2 total ~14 minutes vs my original 45-60 min estimate. Pattern confirmed: estimates for small well-scoped tasks in familiar codebase are 3-5x too high. Bias toward "do it now" for tasks under 10 minutes.

See [[0003-memory-layering]], [[0004-allocator-routing]], [[0005-user-pointer-validation]] for sidebar learnings during this lesson.
