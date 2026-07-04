# Lesson 10 complete — ext2 superblock parsed

User completed Lesson 10 (steps 10.1, 10.2, 10.3, 10.4) in a single
session. The kernel now reads bytes from disk and parses them as an ext2
superblock. magic = 0xEF53 confirmed, block_size = 1024, 128 inodes,
1024 blocks. The filesystem phase has begun.

**What was built (4 commits, one per step):**

| Step | Commit | What landed |
|---|---|---|
| 10.1 | `f7cc83e` | `scripts/make-disk.sh` updated to run `mkfs.ext2 -F` on the disk image. Disk signature `MINIOS_DISK_v0\n` from Lesson 5 is now overwritten — Lesson 10 verifies the ext2 magic instead. Required `brew install e2fsprogs` (mkfs.ext2 isn't in macOS by default). |
| 10.2 | `380a52d` | New `src/ext2.rs` module. `Superblock` struct holds 10 fields we care about. `Superblock::parse(&[u8])` reads each field via `from_le_bytes` at its spec offset. Constants: `EXT2_MAGIC = 0xEF53`, `EXT2_ROOT_INO = 2`. |
| 10.3 | `34a7e78` (combined) | `read_superblock(dev)` reads sectors 2 and 3, concatenates into 1024 bytes, parses. |
| 10.4 | `34a7e78` (combined) | Test block in main.rs: parse, pretty-print, assert magic/block_size/first_data/state/inodes/blocks all match expected values. |

**Key technical landings:**

- **Bytes are bytes; the disk doesn't know it's ext2.** ext2 is a parsing convention. The kernel reads raw sectors via `read_sector` (Lesson 9) and applies the ext2 layout to find meaning. Two programs reading the same disk can see different things — one sees random bytes, another sees a tree of files. The difference is whether they know the convention.

- **from_le_bytes vs repr(C) transmute.** Picked from_le_bytes. Advantages: no alignment assumption (slice doesn't need 4-byte alignment), explicit endianness documentation, no unsafe. ext2 is little-endian, x86 is little-endian, but the same code would work on a big-endian architecture without modification. The closure helpers (`u32_at`, `u16_at`) hide the `try_into().unwrap()` boilerplate.

- **Two-sector read for the 1024-byte superblock.** The superblock is 1024 bytes; `read_sector` returns 512. Read sectors 2 and 3, concat into a stack array. Future optimization: a `read_bytes(offset, len)` primitive that handles multi-sector reads internally.

- **Magic = 0xEF53 is the load-bearing check.** The first thing the parser does after parsing is `is_ext2()` — if the magic doesn't match, we're either reading the wrong disk or the disk isn't ext2. Without this check, garbage input could produce plausible-looking but wrong field values.

**Discoveries:**

- **s_state is u16, not u32.** My first cut had it as u32, which slurped in the next field (`s_errors`) too — got value 65537 (0x10001 = state 1 | errors 1<<16) instead of 1. Fixed by reading as u16 per spec. Lesson: verify field widths against the spec, don't assume.

- **macOS doesn't have mkfs.ext2 by default.** Used `brew install e2fsprogs`. The script tries multiple paths (`command -v`, brew location, Linux fallback) so it works on both macOS (with brew) and Linux.

- **Disk sector 0 is mostly zeros after mkfs.ext2.** ext2's boot block is reserved but not used by default. Lesson 8's signature check (`MINIOS_DISK_v0\n`) needed to be removed since the disk is no longer raw with a signature.

- **mkfs.ext2 reports `1024 1k blocks and 128 inodes`** for a 1 MB disk. Matches the superblock's parsed values exactly. mkfs is consistent with what the kernel reads back.

**Zone of proximal development now:**

- The kernel can parse the ext2 superblock. Next steps require looking up inodes (in the inode table) and walking directory entries.
- Ready for Lesson 11 (page cache, deferred) OR Lesson 12 (ext2 inode lookup). Per the user's earlier decision, we'll continue the ext2 path and come back to page cache later.
- Lesson 12 needs the Block Group Descriptor Table (BGDT) to find where the inode table lives, then read inode 2 (root directory by ext2 convention) from that table.

**Calibration data — Lesson 10 total wall clock:**

| Step | Time | Notes |
|---|---|---|
| 10.1 | ~5 min | script update + brew install + verification |
| 10.2 | ~5 min | struct + parser, mostly straightforward from spec offsets |
| 10.3 + 10.4 | ~7 min | read_superblock function + test block + state-width bug fix |
| **Total** | **~17 min** | Four focused commits |

Easiest filesystem lesson. The hard parts (read_sector, descriptor chains) were done in earlier lessons. ext2 superblock parsing is mechanical — read offsets from the spec, write from_le_bytes calls.

**Sidebars during Lesson 10:**

- **Block terminology collision.** User noticed "block" used in both virtio-blk (512 B device blocks) and ext2 (1024 B filesystem blocks). Renamed `read_block` → `read_sector` throughout the project for clarity. Per Linux convention: block = filesystem unit, sector = device unit. Layer name "block layer" stayed (it describes the layer's role, not function granularity).
- **Historical context + FAT comparison.** User asked for context — where ext2 fits, comparison to ext3/ext4/FAT. Added Historical context section to the lesson covering the Extended family tree (ext → ext2 → ext3 → ext4, all backwards compatible), why ext2 looks the way it does (1993 hardware constraints), and how FAT differs (simpler, no journal/permissions/links, lives in UEFI partitions and USB sticks today).

See [[0012-lesson-9-complete]] for the prior lesson record.
