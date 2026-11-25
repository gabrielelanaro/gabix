# Lesson 7 complete — virtqueues allocated, configured, and ready

User completed Lesson 7 (all five sub-steps 7.1 through 7.5) in a single
session. Kernel now owns a fully-configured split virtqueue: three
regions in physically-contiguous RAM, queue accepted by the device,
descriptor alloc/free working, ring indices advancing on submit, and
the kick + poll mechanics in place. The queue is *configured* but
not yet *doing* real I/O — Lesson 8 sends the first valid request.

**What was built (5 commits, one per step):**

| Step | Commit | What landed |
|---|---|---|
| 7.1 | `49a9f79` | `VirtQueue::new(queue_size)` — three 4 KiB frames from the frame allocator, one each for descriptor table / avail ring / used ring. Zeroed. Stashes both virt and phys addresses. |
| 7.2 | `b1d462e` | `VirtioBlk::configure_queue(&VirtQueue)` — writes queue_select/queue_size/queue_desc/queue_avail/queue_used/queue_enable in spec order. Read-back of queue_enable=1 confirms acceptance. New u16/u64 MMIO helpers. |
| 7.3 | `3fd5df2` | `alloc_descriptor` / `free_descriptor` — Vec<u16> free-list stack, LIFO. `write_descriptor(idx, addr, len, flags, next)` helper. `VirtqDesc` repr(C) struct + flag constants (F_NEXT, F_WRITE). |
| 7.4 | `b6c8225` | `submit_chain(head)` — reads avail->idx, writes head into ring[idx % N], `fetch_add(1, Release)` on idx. The Release ordering is the memory barrier the spec mandates. |
| 7.5 | `a5ec3f7` | `VirtioBlk::kick()` — writes queue index to notify register via `notify_virt + queue_notify_off × multiplier`. `VirtQueue::poll_completion()` — Acquire-loads used->idx, returns (id, len) when advanced. Extended cap parsing for notify_off_multiplier. |

**Key technical landings:**

- **Per-region single-frame allocation.** For queue_size <= 64, each of the three regions fits in one 4 KiB frame (desc=1024 B, avail=134 B, used=518 B). The frame allocator returns page-aligned, physically-contiguous frames by construction — we get the spec's alignment requirements for free without a custom allocator. For larger queues we'd need multi-frame allocations; we don't need them yet.

- **Direct map for virt → phys translation.** Each region's virtual address is `phys + phys_offset()`. No new page-table mappings needed for the regions themselves (the bootloader's direct map covers all of RAM). MMIO regions (like notify) still need explicit `map_mmio` because the direct map doesn't cover device memory.

- **AtomicU16 for the ring indices.** The avail and used rings' `idx` fields are the publication points between kernel and device. Using `AtomicU16` with `Release` (on submit) and `Acquire` (on poll) is the spec-compliant way to enforce the memory ordering — Rust lowers these to the right barrier instructions per architecture. On x86, often a single locked instruction with implicit full barrier; on ARM, an explicit `dmb`.

- **Notification capability extension.** The notification cap has an extra `notify_off_multiplier` field at PCI config offset 16 (beyond the standard virtio_pci_cap body). The cap parsing now reads this; VirtioBlk stashes it alongside `notify_virt` for use in `kick()`. The multiplier is per-device, not per-queue; per-queue offset comes from `COMMON_QUEUE_NOTIFY_OFF` in common config.

- **What "works" means in Lesson 7.** We verified the queue mechanics end-to-end (allocation, configuration accepted, free-list LIFO behavior, ring indices advance on submit, kick hits the right address, poll returns None when there's nothing to report). We did NOT yet verify actual disk I/O — the test descriptor has bogus data (addr=0, len=0, flags=0). Lesson 8 sends a real virtio-blk request (3-descriptor chain: header + data buffer + status byte).

**Discoveries:**

- **QEMU's virtio-blk default queue max is 256.** We negotiate down to 64. The smaller size keeps the "everything fits in one frame per region" assumption valid and is plenty for our single-process kernel.

- **QEMU's notify_off_multiplier is 4.** With `queue_notify_off = 0` for queue 0, the kick writes at offset 0 of the notification region. The multiplier matters more when there are multiple queues (network cards have separate RX/TX queues, each at a different notify offset).

- **x86 makes the barriers nearly free.** `fetch_add(1, Release)` on x86 compiles to a `lock xadd` instruction which is already fully ordered. The Acquire load on poll is just a regular load (x86's TSO model gives Acquire for free). The barriers matter most on ARM, where they emit `dmb`. Writing portable code means we get correctness on both.

- **Cap parsing had to grow.** Lesson 6's `VirtioCap` had four fields (cfg_type, bar, offset, length). Lesson 7 added `notify_off_multiplier` (read from PCI config offset cap+16). Only meaningful for the notification cap, but the field is present on all of them for layout simplicity.

**Zone of proximal development now:**

- All the queue mechanics absorbed: allocation, configuration, ring submission with proper barriers, kick + poll. The vocabulary (descriptor, available ring, used ring, F_NEXT, F_WRITE, head index, notification register) is solid.
- The Release/Acquire pattern clicked — user asked "what is a memory barrier?" and "what does 'visible to the device' mean?" mid-session, both answered and demonstrated with the actual fetch_add / load code.
- Ready for Lesson 8 (Keystone 1): the first real disk read. Build a virtio-blk request header (16 bytes: type, reserved, sector, etc.), chain three descriptors (header + data buffer + status byte), submit, kick, poll, verify the 512-byte buffer contains `MINIOS_DISK_v0` + padding.

**Calibration data — Lesson 7 total wall clock:**

| Step | Time | Notes |
|---|---|---|
| 7.1 | ~6 min | VirtQueue struct + alloc, plus the frame/phys_offset helpers wiring |
| 7.2 | ~4 min | configure_queue + u16/u64 MMIO helpers |
| 7.3 | ~3 min | free list + write_descriptor + VirtqDesc |
| 7.4 | ~4 min | submit_chain, Release ordering, index arithmetic |
| 7.5 | ~8 min | kick + poll + cap parsing extension for notify_off_multiplier |
| **Total** | **~25 min** | Five focused commits |

Longer than Lessons 5 (~17 min) and 6 (~9 min) because the moving parts multiplied: three regions, two barriers, three new MMIO widths, and the notification cap parsing extension. Per-step work stayed manageable.

**Sidebars during Lesson 7:**

- **What is a memory barrier?** User asked the question directly. Explained store buffers / cache hierarchy / "visible to the device" as a propagation chain, then mapped Rust's `Release` (publish writes) / `Acquire` (see published writes) onto concrete CPU instructions (mfence / dmb).
- **Read vs write — what actually differs?** User noticed the T0-T8 flow was direction-agnostic and asked which one it was for. Clarified: the flow is the same shape; only the F_WRITE flag on the data descriptor flips. Added a side-by-side comparison block to the lesson showing the 3-descriptor chains for read vs write.
- **Cache coherence deferred.** User flagged cache coherence (MESI) and store buffer mechanics as a sidebar to revisit later — noted in the session log. Natural pickup point: Lesson 10 (page cache) or any time DMA coherency on real hardware matters.

**Teaching workflow:**

- **byte-layout.css reusable component.** Built during this lesson (originally for the descriptor entry visual). Now used for the descriptor entry, the available ring, the used ring, AND the common config queue fields. Will reuse for ext2 superblock, ext2 inode, ELF header in upcoming lessons.
- **Per-snippet filename captions maintained** in the tour. Every code block in the tour has a `.snippet-meta` header showing the file + what's being shown.
- **Field-reference block under each byte-layout.** Each visual is paired with a definition-list-style block giving per-field purpose + concrete examples with addresses and values. Pushed the lesson's explanatory depth past what earlier lessons had.

See [[0009-lesson-6-complete]] for the prior lesson record.
