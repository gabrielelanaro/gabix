# Lesson 12 complete — ext2 inode lookup works

User completed Lesson 12 (all five steps 12.1 through 12.5) in a single
session. The kernel can now read any inode by number — given N, walk the
BGDT, find the inode table, index in, parse the 128-byte struct. Inode 2
(the root directory by ext2 convention) verified as a directory at block
40 with mode 0x41ED.

**What was built (2 commits):**

| Step | Commit | What landed |
|---|---|---|
| 12.1 | `ba06f20` | Extended Superblock with `inodes_per_group` (u32 @ 40) and `inode_size` (u16 @ 88). Print updated. |
| 12.2-12.5 | `571ebba` | `BlockGroupDescriptor` struct (32 bytes) + parser. `Inode` struct (128 bytes parsed) + parser. `read_block_group_descriptor`, `read_inode` functions. Constants `S_IFMT`/`S_IFREG`/`S_IFDIR`/etc. Test block in main.rs verifies inode 2 is a directory. |

**Key technical landings:**

- **The lookup chain is mechanical once the math is right.** Inode N → group (N-1)/inodes_per_group → block group descriptor → bg_inode_table → byte offset = bg_inode_table × block_size + (N-1) % inodes_per_group × inode_size. Five steps from number to struct, each one yielding the next one's input.

- **Field widths matter.** Lesson 10 had a bug where `s_state` was u32 instead of u16, slurping in the next field. Lesson 12 was careful: every field width verified against the spec.

- **Two-sector read pattern.** Both `read_block_group_descriptor` (32 bytes that might straddle sectors) and `read_inode` (128 bytes that definitely straddles sectors when inode_size > 256 and offset isn't aligned) read two sectors and concat into a 1024-byte buffer. Future optimization: a `read_bytes(offset, len)` primitive.

- **i_mode decoding via mask.** `file_type() = mode & S_IFMT` gives the top 4 bits. Compare against S_IFREG/S_IFDIR/S_IFLNK constants. The bottom 12 bits are Unix permissions (rwxrwxrwx + setuid/setgid/sticky).

- **block_pointers[15] modeling.** We parse all 15 but only print the direct ones (12) plus the indirect/double/triple pointers as numbers. Lesson 13 will walk a directory's first direct block to list entries.

**Discoveries:**

- **`bg_inode_table = 8`, not 5 as the lesson visual estimated.** The visual was based on the assumption that inode_size = 128 (8 inodes per block, 16 blocks of inode table). Modern mkfs uses `inode_size = 256` by default, which means 4 inodes per block, 32 blocks of inode table. This pushes the data block section later. The code uses `bg_inode_table` directly from the parsed BGD, so the actual value doesn't matter — we follow the disk's own map.

- **`inode_size = 256` is the modern default.** Not the spec's 128-byte minimum. Means each inode has 128 bytes of extended-attribute space at the end (we ignore it). Lesson 12 parses only the first 128 bytes regardless of `s_inode_size`.

- **`used_dirs_count = 2` for our disk.** mkfs creates two directories by default: `/` (inode 2) and `/lost+found` (inode 11). The allocator uses this count to spread new directories across block groups on multi-group disks.

- **Root directory at block 40.** With `inode_size = 256`, the inode table runs from block 8 to block 39 (32 blocks for 128 inodes). Block 40 is the first data block — used by inode 2 for its directory entries.

- **`i_blocks` reports sectors, not ext2 blocks.** Inode 2 reports `blocks = 2` (sectors), which is 1 ext2 block. The spec uses 512-byte sectors as the unit for compatibility with the Unix stat interface, regardless of filesystem block size.

**The bigger picture:**

- The kernel can read any inode by number. Filesystem-level operations now have a foundation: directories can be walked (Lesson 13), file data can be read (Lesson 14), the root filesystem can be mounted (Lesson 14).
- ext2 is just a parsing convention. The kernel reads raw sectors via `read_sector` (Lesson 9) and applies the ext2 layout. No magic — just byte offsets and field widths.

**Zone of proximal development now:**

- Ready for Lesson 13 (ext2 directory walk). Read block 40 (root dir's data), parse the variable-length dirent entries (inode + name + type), list the contents.
- After Lesson 13: Lesson 14 (mount ext2 as root). Replace the hardcoded `ROOT_INODE` lazy_static with a live ext2 mount.
- After Lesson 14: ELF loader (Lesson 15) → exec syscall (Lesson 16). The mission goal — load and run an ELF from disk — is in reach.

**Calibration data — Lesson 12 wall clock:**

| Step | Time | Notes |
|---|---|---|
| 12.1 | ~3 min | Two more fields on Superblock |
| 12.2 | ~4 min | BGD struct + parser, mirrors Superblock pattern |
| 12.3 | ~3 min | read_block_group_descriptor, sector math |
| 12.4 | ~6 min | Inode struct (15 block pointers!) + read_inode |
| 12.5 | ~3 min | Wire into main.rs, assertions, print |
| **Total** | **~20 min** | Two commits (12.1 separate, 12.2-12.5 bundled because they share the print/assert wiring) |

**Sidebars during Lesson 12:**

- **Visual improvements + global library extraction.** User pushed for: (a) graphical disk layout instead of text, (b) interactive click-to-reveal on every visual element, (c) reusable component library so future lessons get this for free. Built `assets/explorable.css` + `assets/explorable.js` for the click-to-reveal pattern (cells with `data-reveals` + `<template>` elements). Added container queries to `byte-layout.css` so small cells auto-collapse to size badges. Added instant CSS tooltips via `::before { content: attr(title) }`. Added "show all / collapse" toggle for overview mode. Saved as `feedback_interactive_lessons` memory.
- **Contrastive-opener ban expansion.** Lesson 12 prep surfaced the pattern "where calls stop being X and start being Y" — same tic family as previously banned. Updated memory + CLAUDE.md writing-style section to list more variants.
- **read_block → read_sector rename.** User noticed "block" used in both virtio-blk (512 B device blocks) and ext2 (1024+ B filesystem blocks). Renamed throughout the project. Per Linux convention: block = filesystem unit, sector = device unit (always 512 B).
- **Block-group vs global structures.** User asked whether BGDT is also copied like the superblock. Explained: superblock + BGDT are global (one primary + backups in select groups); bitmaps + inode table + data blocks are per-group. The pair travels together because BGDT format depends on superblock fields.

See [[0013-lesson-10-complete]] for the prior lesson record.
