# Lesson 6 complete — virtio device initialization, device is live

User completed Lesson 6 (all four sub-steps 6.1 through 6.4) in a single
session. Kernel now parses the virtio PCI capability list, maps MMIO
regions, and walks through the full virtio modern-transport handshake
end-to-end. The device is in DRIVER_OK state — operational, ready for
virtqueue setup in Lesson 7.

**What was built (across 4 commits, one per step):**

| Step | Commit | What landed |
|---|---|---|
| 6.1 | `fc21746` | `src/virtio.rs`: VirtioCap struct, parse_virtio_capabilities walks PCI cap list via next_ptr, identifies vendor-specific caps (cap_id=0x09) and decodes virtio sub-type + BAR + offset + length |
| 6.2 | `79e043e` | `src/memory.rs`: phys_offset() and map_mmio() (PRESENT \| WRITABLE \| NO_CACHE \| NO_EXECUTE). `src/pci.rs`: bar_base() decode helper. `src/virtio.rs`: VirtioBlk struct with volatile MMIO access (read/write_common_u8/u32), probe() that finds + maps, reset_and_acknowledge() |
| 6.3 | `ab8ff4b` | negotiate_features() — reads both halves of device features, claims VIRTIO_F_VERSION_1 only, writes driver features, sets FEATURES_OK, asserts the device accepted |
| 6.4 | `d5f03d3` | set_driver_ok() — writes ACK \| DRIVER \| FEATURES_OK \| DRIVER_OK (0x0F), asserts no FAILED or NEEDS_RESET, prints "device is live" |

**Key technical landings:**

- **MMIO mapping** — bootloader's direct map covers RAM only, not device regions. Added `map_mmio(phys, size)` that explicitly creates page table entries with NO_CACHE (load-bearing — without it the CPU would cache device register reads/writes and break the protocol).

- **Volatile access** — `core::ptr::{read_volatile, write_volatile}` for all MMIO. Without volatile, the compiler would elide or reorder loads/stores. Device registers have side effects; the access has to happen exactly as written.

- **Capability list traversal** — start at config offset 0x34, follow next_ptr chain. For each vendor-specific cap (cap_id=0x09), parse the sub-header: cfg_type (1=common, 2=notify, 3=ISR, 4=device, 5=PCI), BAR index, offset within BAR, region length.

- **Handshake sequence** — reset → ACKNOWLEDGE → DRIVER → read device features → write driver features → FEATURES_OK (then re-read to confirm acceptance) → DRIVER_OK. Each phase has a distinct purpose: ACK vs DRIVER distinguishes "I see you" from "I have a driver"; FEATURES_OK is the spec-mandated negotiation checkpoint; DRIVER_OK is the final "go" signal.

- **Two-phase feature space** — features are 64 bits. Low 32 are device-specific (virtio-blk features in our case), high 32 are transport features (VIRTIO_F_VERSION_1 etc.). Access via *_feature_select then *_feature.

**Discoveries:**

- **QEMU's virtio-blk advertises a rich feature set**: device features low = 0x30006E54 (full virtio-blk suite: SEG_MAX, GEOMETRY, BLK_SIZE, FLUSH, CONFIG_WCE, DISCARD, WRITE_ZEROES, SECURE_ERASE). High = 0x00000101 (VIRTIO_F_VERSION_1 + 1 other transport feature). We accept only VERSION_1 for now; real Linux claims more for performance and correctness.

- **SeaBIOS pre-initializes the device**: "device_status before reset: 0x0F" is normal — QEMU's firmware ran the handshake during boot. Our reset clears it; we then walk through cleanly. Means the device works fine on a fresh boot even without our driver touching it, but we reset for explicit control.

- **Capability order in the list is reversed from logical order**: PCI caps are LIFO (newest first). The common config cap appears last in the linked list even though we read it first conceptually.

**Zone of proximal development now:**

- PCI config space, MMIO access, capability list traversal, volatile semantics, feature negotiation pattern all absorbed
- The virtio modern transport's structure is understood: capability list → region pointers → MMIO access → handshake → operational
- Ready for Lesson 7: virtqueues. The shared-memory ring buffer mechanism (descriptor table, available ring, used ring) that we'll use to actually submit disk requests.

**Calibration data — Lesson 6 total wall clock:**

| Step | Time | Notes |
|---|---|---|
| 6.1 | ~3 min | cap list parsing, no MMIO yet |
| 6.2 | ~3 min | heaviest step — map_mmio, bar_base, VirtioBlk struct + probe + first phase |
| 6.3 | ~2 min | feature negotiation |
| 6.4 | ~1 min | one method |
| **Total** | **~9 min** | Four focused commits |

Pattern holds: small well-scoped steps in familiar territory stay in the 1-3 minute range. Lesson 6 was the biggest yet (virtio is conceptually heavy) but the per-step work was small once the design was clear.

**Sidebars during Lesson 6:**

- Features catalog: user asked what features exist beyond VIRTIO_F_VERSION_1 and VIRTIO_BLK_F_BLK_SIZE. Explained two categories (transport features at bits 32+, device-specific at 0-31) with examples for virtio-blk (FLUSH, DISCARD/TRIM, WRITE_ZEROES, SECURE_ERASE) and other virtio device types (virtio-net's checksum offload, TSO, multiqueue; virtio-gpu's 3D acceleration).
- Why the handshake has so many bits: each is a checkpoint with distinct semantics. ACK vs DRIVER = "I see you" vs "I have a driver for you". FEATURES_OK with re-read = negotiation protocol. DRIVER_OK = final go. Lets the device refuse to advance at any stage.

**Teaching workflow:**

- Tour convention maintained throughout: per-step sections with file-captioned code snippets, verification output for each phase.
- Click-expandable concept cards in the lesson: capability IDs list, why-split-the-handshake explanation, modern vs legacy vs transitional distinction.

See [[0008-lesson-5-complete]] for the prior lesson record.
