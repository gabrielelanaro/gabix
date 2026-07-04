# Lesson 9 complete — block layer abstraction

User completed Lesson 9 (all three sub-steps 9.1, 9.2, 9.3) in a single
short session. The kernel now exposes a clean `read_block(sector)` API
that hides the full virtio-blk descriptor chain + DMA + submit/kick/poll
dance. ext2 (Lesson 11) can plug into this without knowing anything
about virtio.

**Note on commit granularity:** Lesson 9 was committed as a single
`6875d9b` rather than per-step. The three steps (refactor ownership,
add method, exercise it) form one cohesive change — splitting would
leave intermediate commits that don't build cleanly (e.g., a VirtioBlk
with an unused `queue` field; a read_block method with no caller).
Lesson 7 and 8's per-step split worked because each step was
independently valuable. Lesson 9's value comes from the refactor
*plus* the new method together.

**What was built (1 commit):**

| Step | What landed |
|---|---|
| 9.1 | Added `queue: Option<VirtQueue>` field to `VirtioBlk`. New `setup_queue(queue_size)` method that allocates + configures in one call. New `queue_mut()` accessor for test code that still wants to poke the queue directly. |
| 9.2 | Added `read_block(&mut self, sector: u64) -> [u8; 512]` method. Internally: allocate BlkBuffers → write VirtioBlkReq header → build 3-descriptor chain → submit_chain → kick → poll loop → assert status == OK → free descriptors → copy bytes into stack array → return. Changed `build_read_chain` to return `(head, mid, tail)` so callers can free all three. |
| 9.3 | Tested by calling `read_block(0)`, `read_block(1)`, `read_block(2)`. Sector 0 has the MINIOS_DISK_v0 signature; sectors 1 and 2 are all zero (no filesystem yet). All three reads succeed and return the expected bytes. |

**Key technical landings:**

- **The block layer boundary is a single function.** `read_block(sector)` is the API contract. Everything above (ext2, future filesystem code) calls it. Everything below (virtio-blk driver internals) is encapsulated. This is exactly where Linux draws the line too — `submit_bio` is the entry point.

- **VirtioBlk owning VirtQueue is a clean factoring.** Previously `virtio_dev` and `queue` were separate locals in main.rs, and any function wanting to do I/O needed both passed in. With ownership concentrated, the API shrinks to one argument: the sector number. The queue becomes an implementation detail.

- **`[u8; 512]` returned by value is the right type.** Stack-allocated, RVO-optimized (caller's slot is the destination, no intermediate copy), 8 cache lines even if copied. Callers get a self-contained array they can read or drop freely. `Vec<u8>` would heap-allocate per call (worse); `&[u8]` would force keeping the DMA buffer alive (lifetimes get hairy).

- **Borrow checker pattern that worked:** `self.queue.as_mut().unwrap()` at each step rather than binding `let queue = self.queue.as_mut().unwrap()` once. The latter would lock the borrow for the whole function and conflict with `self.kick()`. Each chained call releases the borrow at the end of the statement.

**Discoveries:**

- **Per-call allocation is wasteful but fine for boot.** Each `read_block` allocates 3 fresh frames (12 KB) for header/data/status and never frees them. ext2 mount will read ~50 blocks; that's 600 KB of leaked frames. On a 128 MB QEMU config, fine. Real Linux uses a slab pool for `struct request` and pre-allocated bios; we'll get there in a future optimization lesson.

- **The Lesson 7/8 inline test block was getting unwieldy.** Lesson 9 was a chance to consolidate: kept the size assertion (8.1, no queue access needed) and the free-list smoke test (7.3, useful primitive verification). Dropped the full inline chain/submit/kick/poll/verify block — `read_block` exercises all of it, so re-verifying it inline is redundant.

- **`build_read_chain` returning `(head, mid, tail)` instead of just `head`.** Necessary for proper descriptor cleanup. The chain is 3 descriptors linked by `next` pointers; the head's index isn't enough to find mid and tail without walking the chain via volatile reads. Returning all three is simpler and faster.

**Zone of proximal development now:**

- The disk primitives are complete. The kernel can read any sector by number, getting back a 512-byte array. Filesystem work can begin.
- Ready for Lesson 11: ext2 superblock parser. The superblock lives at byte offset 1024 = sector 2. We already verified `read_block(2)` returns all zeros (no FS yet). Next step is to put an actual ext2 filesystem on the disk image and parse the superblock.
- Lesson 10 (page cache) is deferred — it's an optimization, not on the critical path to "load and run an ELF from disk." Can come back to it later.

**Calibration data — Lesson 9 total wall clock:**

| Step | Time | Notes |
|---|---|---|
| 9.1 + 9.2 + 9.3 | ~12 min | All three steps landed in one work session because they're cohesive. Build first try, QEMU verified all three sectors. |

Easiest lesson since Lesson 5. The refactor is mechanical; the new method is a paste-together of Lesson 8's inline code; the test is three calls plus assertions.

**Sidebars during Lesson 9:**

- **Contrastive opener tic.** User flagged the lesson's opening sentence ("where calls stop being about files and start being about blocks") as the same LLM-ism family previously banned. Fixed the specific instance, expanded the saved memory `feedback_no_contrastive_openers.md` to list more variations ("stop being X / start being Y", "no longer X, now Y", "less about X, more about Y", "shift from X to Y"), and added a Writing style section to `CLAUDE.md` so future sessions inherit the rule.

See [[0011-lesson-8-complete]] for the prior lesson record.
