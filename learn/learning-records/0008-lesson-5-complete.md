# Lesson 5 complete — PCI bus enumeration, virtio-blk identified

User completed Lesson 5 (all four sub-steps 5.1 through 5.4) in a single
session. Kernel now walks bus 0, lists every PCI device QEMU exposes,
identifies the virtio-blk specifically, and decodes its BAR layout.

**What was built (across 4 commits, one per step):**

| Step | Commit | What landed |
|---|---|---|
| 5.1 | `7ec8c03` | `scripts/make-disk.sh` (raw 1MB image with `MINIOS_DISK_v0\n` signature), `scripts/run.sh` (QEMU launch with virtio-blk-pci attached), `.gitignore` for `/disk.img` |
| 5.2 | `aaff31b` | `src/pci.rs`: `CONFIG_ADDRESS`/`CONFIG_DATA` port constants, `make_address` bit-packing helper, `config_read` and `config_write` via `Port::new` + `read`/`write` from `x86_64` crate. Smoke test reads host bridge vendor and asserts it's Intel (0x8086) |
| 5.3 | `738f033` | `class_name` helper for human-readable class labels, `enumerate()` walks bus 0 × 32 devices × 8 functions, prints vendor/device/class for every present slot |
| 5.4 | `e05b0de` | `find_virtio_blk()` accepts both transitional (0x1001) and non-transitional (0x1042) device IDs. `print_bars()` decodes all 6 BARs with type info (memory vs I/O, 32-bit vs 64-bit, prefetchable) and correctly handles 64-bit BARs that consume two slots |

**Discoveries:**
- **QEMU exposes the transitional virtio-blk device ID (0x1001), not the non-transitional (0x1042)** the lesson text expected. Both are virtio-blk — transitional means the device also supports the legacy transport for backward compatibility. Modern Linux works with either. Code accepts both IDs.
- **virtio-blk BAR layout in QEMU**: BAR0 = legacy I/O ports at 0xC000, BAR1 = MSI-X region at 0xFEBF1000, BAR4-BAR5 = modern transport region (64-bit prefetchable memory at 0xFE000000). Lesson 6 will read the capability list to find which offsets within BAR4 hold common config, ISR, and notifications.
- **PCI bridge hierarchy explained**: CPU → host bridge (00:00.0) → PCI bus → [PCI devices] → ISA bridge (00:01.0) → ISA bus → [legacy peripherals: PIC, PIT, RTC, serial ports]. When we walk "bus 0" we only see the PCI side; the PIC and PIT that mini-os already uses are on the far side of the ISA bridge, at fixed I/O ports, not at PCI slots.

**Zone of proximal development now:**
- PCI config space access, port I/O (Port<u32> pattern), bus walk, BAR decoding all absorbed
- Vendor/device ID concept, class code concept, I/O vs memory BAR distinction absorbed
- Ready for Lesson 6: virtio device initialization (capability list, handshake: reset → ACKNOWLEDGE → DRIVER → FEATURES_OK → DRIVER_OK)

**Calibration data — Lesson 5 total wall clock:**

| Step | Time | Notes |
|---|---|---|
| 5.1 | ~100s | disk image creation, run.sh script, QEMU verification |
| 5.2 | ~95s | pci.rs module, config_read/write, smoke test |
| 5.3 | ~127s | class_name lookup + enumerate function |
| 5.4 | ~60s | fastest step yet — pure additive on existing patterns |
| **Total** | **~7 min** | Four focused commits |

**Pattern confirmed again:** estimates for small well-scoped tasks in
this codebase stay in the 1-3 minute range. The 4-step lesson plan
including concept reinforcement and tour updates took under 7 minutes
of focused work.

**Sidebars during Lesson 5 worth tracking:**
- "Configuration space is mixed — read-only identity fields (vendor, device, class) vs read-write control fields (command, status, BARs)". User pushed back on why config_write is needed.
- Bridge concept explained in depth: host bridge vs ISA bridge vs ACPI PM. Connection to existing mini-os code (PIC at 0x20, PIT at 0x40 are on the far side of the ISA bridge).
- I/O ports as a separate x86 address space (not memory). Accessed via in/out instructions, Port::new + read/write wraps them.

**Teaching workflow developments during Lesson 5:**
- Tour convention maintained: per-step sections with file-captioned code snippets, click-expandable for new concepts (read-only vs writable config fields, BAR explanation, QEMU device inventory)
- "Don't refer to step numbers without saying what the step does" convention held throughout — every step referenced by both number AND a short description of the work

See [[0006-lesson-2-complete]], [[0007-lesson-3-complete]] for prior lesson records.
