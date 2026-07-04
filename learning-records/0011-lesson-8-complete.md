# Lesson 8 complete — Keystone 1: bytes flow from disk

User completed Lesson 8 (all five sub-steps 8.1 through 8.5) in a single
session. Kernel builds a real virtio-blk request — 3-descriptor chain
of header + data buffer + status byte — submits it, kicks the device,
polls for completion, and verifies the 512 bytes that come back match
the `MINIOS_DISK_v0\n` signature written by `make-disk.sh` in Lesson 5.

This is Keystone 1. Everything in Lessons 5–7 was setup; Lesson 8 is the
payoff: bytes flow from disk for the first time.

**What was built (5 commits, one per step):**

| Step | Commit | What landed |
|---|---|---|
| 8.1 | `233f4dd` | `VirtioBlkReq` struct (16 bytes: type/reserved/sector, repr(C)). Constants `VIRTIO_BLK_T_IN/OUT/FLUSH`, `VIRTIO_BLK_S_OK/IOERR/UNSUPP`. `VirtioBlkReq::new_read(sector)` constructor. Asserted size == 16 at boot. |
| 8.2 | `c43b55d` | `BlkBuffers` struct — three DMA-able frames (header / data / status), zeroed. Tracks frame numbers for future cleanup. Methods `write_header`, `read_status`, `read_data_prefix`. |
| 8.3 | `003abcf` | `VirtQueue::build_read_chain(header_phys, data_phys, status_phys) -> u16`. Allocates 3 descriptors from the free list, writes their fields with the F_NEXT / F_WRITE pattern, returns the head index. |
| 8.4 | `d42e82a` (combined with 8.5) | Submit chain → kick → poll loop with 200000-iter cap. Added `VirtQueue::debug_state()` for diagnostics. First poll iteration returned a completion. |
| 8.5 | `d42e82a` (combined with 8.4) | Verify `status == VIRTIO_BLK_S_OK`, `data[0..15] == b"MINIOS_DISK_v0\n"`, `data[15..512]` all zero. Free descriptors back to the free list. |

**Key technical landings:**

- **Two-layer model absorbed.** The descriptor table (Lesson 7) is the universal mechanism — every virtio device uses it. The virtio-blk request header (Lesson 8) is device-specific content that fills one of the buffers a descriptor points at. Same 16 bytes by coincidence; totally unrelated structures.

- **The 3-descriptor chain pattern.** Every virtio-blk request is: header (kernel writes, device reads) → data (direction depends on operation) → status (device writes 0 on success). F_NEXT on descriptors 0 and 1 links them; F_WRITE on descriptors 1 and 2 marks them device-writable. The chain is a singly-linked list of descriptor indices; the device follows `next` pointers from the head.

- **DMA-able buffers via the frame allocator.** The device needs physical addresses. The frame allocator returns page-aligned frames with trivially-computable physical addresses (`frame × 4096`). Wasteful (16 B header in a 4096 B frame) but the simplest correct path. Real Linux uses `kmalloc + dma_map`; that's a future optimization.

- **Polling vs interrupts.** We poll `used->idx` in a tight loop with a generous cap. Real Linux uses MSI-X interrupts — the device raises an interrupt on completion, the kernel gets an async notification. We'll add interrupts later; for now polling is simpler and proves the mechanism.

- **Acquire/Release in action.** The Lesson 7 `submit_chain` (Release on idx store) and `poll_completion` (Acquire on idx load) carried over directly. The Release on submit publishes the descriptor data; the Acquire on poll sees the device's response data after seeing `idx` advance. The pairing worked first try.

**Discoveries:**

- **QEMU's virtio-blk rejects garbage descriptors in a way that blocks the queue.** Original Lesson 7 test submitted a descriptor with `addr=0, len=0, flags=0`. The device never produced a completion for it AND never processed the subsequent valid request in Lesson 8 — `used->idx` stayed at 0 through 200000 poll iterations. Removing the Lesson 7 garbage submission fixed Lesson 8 immediately. The Lesson 7 test was modified to verify alloc/write_descriptor/free without actually submitting — the underlying methods still work; they're exercised by Lesson 8's real request.

- **`used->ring[i].len` reports total bytes written to device-writable descriptors.** For our read chain: 512 (data buffer, F_WRITE) + 1 (status byte, F_WRITE) = 513. The header buffer has no F_WRITE so its 16 bytes don't count. This matches the spec but is easy to misremember.

- **Completion arrives fast.** After removing the Lesson 7 garbage submission, the first poll iteration found the completion. QEMU's virtio-blk backend processes requests synchronously on kick — no measurable delay on the guest side. Real hardware would have actual disk I/O latency (microseconds to milliseconds), and the polling loop would actually spin.

- **No IOMMU needed.** QEMU's emulated virtio-blk is trusted; physical addresses we hand it go straight to guest RAM. Real Linux on real hardware always has the IOMMU on (security: PCI passthrough would be a VM escape without it). Drivers use `dma_map` / `dma_unmap` which abstracts whether an IOMMU is present.

**Zone of proximal development now:**

- All the disk primitives absorbed: PCI discovery, virtio handshake, virtqueue setup, descriptor chain construction, kick + poll. The kernel can read any sector of the disk on demand.
- Ready for Lesson 9: Block layer abstraction. Wrap "submit a read for sector N" into a clean `read_sector(n) -> [u8; 512]` function. Generalizes Lesson 8 into a primitive the ext2 filesystem will sit on.
- After Lesson 9: ext2 superblock parsing (Lesson 11), which is where disk bytes start having filesystem meaning.

**Calibration data — Lesson 8 total wall clock:**

| Step | Time | Notes |
|---|---|---|
| 8.1 | ~3 min | Just a struct + constants |
| 8.2 | ~5 min | BlkBuffers struct + 3 helper methods |
| 8.3 | ~3 min | build_read_chain — 3 write_descriptor calls |
| 8.4 | ~10 min | Most of this was debugging the no-completion issue; added debug_state diagnostic, traced it to Lesson 7's garbage submission |
| 8.5 | ~3 min | Verification — status byte + signature check + descriptor freeing |
| **Total** | **~25 min** | Five focused commits |

The Lesson 7 garbage-descriptor issue was the unexpected time sink. Without it, Lesson 8 would have been ~15 minutes end-to-end. Captured as a discovery above so future lessons avoid similar pitfalls.

**Sidebars during Lesson 8:**

- **What is an IOMMU?** Explained as the device-side analog of the CPU's MMU — translates DMA addresses, enforces isolation. Why it matters: PCI passthrough security, old-device support, driver bug containment. mini-os has none (QEMU trusted); real Linux always has one. Added to GLOSSARY.
- **Two-layer model (descriptor table vs request header).** User got confused that Lesson 8 introduces a "header" after Lesson 7's "descriptor." Clarified: the descriptor table is the universal mechanism (every virtio device uses it the same way); the header is just the content of one buffer that descriptor[0] points at. Two unrelated structures that happen to both be 16 bytes.
- **Buffer alignment.** Spec doesn't require any alignment for buffers — they can be at any byte address. The frame allocator returns 4096-byte-aligned addresses, way more than needed. The strict alignment requirements are for the virtqueue itself (page-aligned).
- **Acquire on read.** User asked what `Acquire` ordering does on the load side. Explained: pairs with the device's Release store; guarantees the kernel's subsequent reads (of the used ring entry) happen after seeing the idx advance. Without Acquire, the CPU could reorder the ring-entry read before the idx load and read stale data.
- **How Acquire/Release translate to assembly.** On x86-64: `Acquire` is free (regular `movzx` load), `Release` on `fetch_add` is `lock xadd` (full barrier). The annotations matter for portability (ARM needs explicit `dmb ish` barriers) and for the compiler (prevents reordering of plain reads/writes across the atomic point).

See [[0010-lesson-7-complete]] for the prior lesson record.
