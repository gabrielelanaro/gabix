# Lesson 13 complete — ext2 directory walk works

User completed Lesson 13 (all five steps 13.1 through 13.5) in a single
session. The kernel can now read a directory's contents, list its
entries, and look up files by name. The ext2 reading layer is complete
— everything needed for path resolution is in place.

**What was built (1 commit):**

| Step | What landed |
|---|---|
| 13.1 | `Dirent` struct (inode, rec_len, name_len, file_type, name) + `parse_at(bytes, offset)` |
| 13.2 | `read_block(dev, block_n)` — extracted from inline 2-sector reads (was duplicated in superblock + inode readers) |
| 13.3 | `walk_directory(dev, dir_inode) -> Vec<Dirent>` — walks the linked list inside direct blocks |
| 13.4 | `lookup_in_dir(dev, dir_inode, name) -> Option<u32>` — path-resolution primitive |
| 13.5 | Test in main.rs: walk root, verify 3 entries, lookup lost+found → 11, missing → None |

**Key technical landings:**

- **Directories are just files with structured data.** A directory's data block contains dirents (linked list via `rec_len`). The kernel walks the list, comparing names. No magic — the same `read_block` primitive that reads any file data.

- **Variable-length linked-list traversal.** Each entry's `rec_len` field says "skip this many bytes to reach the next entry." This accommodates variable name lengths AND lets dead entries (deleted files) be skipped without compacting. The walker just advances by `rec_len` until it reaches the end of the block.

- **Dead entries stay in place.** When a file is deleted, the inode is set to 0 but the dirent slot remains. The previous entry's `rec_len` is bumped to skip over the dead slot. This avoids the cost of shifting entries left under concurrent access. The walker filters by `inode != 0`.

- **`file_type` byte is a hint.** Same encoding as `i_mode`'s top 4 bits, shifted right by 12. Lets the walker know "is this a directory?" without reading the inode. Saves one disk read per entry during traversal.

- **`read_block` extraction was overdue.** Lesson 10's `read_superblock`, Lesson 12's `read_inode`, and now Lesson 13's directory walker all needed "read 1024 bytes starting at block N." Pulled out as `read_block(dev, block_n) -> [u8; 1024]`. Future block-size-aware version would replace the hardcoded 1024.

**Discoveries:**

- **Root directory's three entries confirmed on QEMU:**
  - `.` → inode 2 (self-reference, conventional)
  - `..` → inode 2 (root's parent is itself — eliminates a base case in path resolution)
  - `lost+found` → inode 11 (first user inode after reserved 1-10)

- **`lookup_in_dir` returns `Option<u32>`** — the natural type for "found it / didn't." Path resolution in Lesson 14 will use this and propagate None up the call chain.

- **Dead-entry handling is correct.** After mkfs, all 3 entries are live (no deletions yet). But the parser handles `inode == 0` correctly by filtering — verified by code review, not yet by test (would need to delete a file from the disk image).

**The bigger picture:**

- The kernel has the full ext2 reading layer: superblock, BGDT, inode table, inode struct, directory entries. Anything you can do with a read-only ext2 filesystem is now possible.
- Path resolution is `for component in path.split('/') { inode = lookup_in_dir(inode, component) }`. That's Lesson 14.
- After Lesson 14 (mount ext2 as root), the kernel replaces the in-memory `ROOT_INODE` lazy_static with a live ext2 mount. Files added to the disk image become accessible via the existing VFS layer.

**Zone of proximal development now:**

- Ready for Lesson 14 (mount ext2 as root). Bridge `src/ext2.rs` with `src/fs.rs` — make `resolve_path` use ext2 lookups instead of the in-memory ROOT_INODE.
- After Lesson 14: ELF loader (Lesson 15) → exec syscall (Lesson 16). The mission goal — load and run an ELF from disk — is within reach.
- Lesson 11 (page cache) still deferred. Will come back after the full path-to-exec chain works.

**Calibration data — Lesson 13 wall clock:**

| Step | Time | Notes |
|---|---|---|
| 13.1 | ~4 min | Dirent struct + parse_at, similar pattern to Inode |
| 13.2 | ~2 min | Extracted read_block, pure refactor |
| 13.3 | ~5 min | walk_directory, the linked-list loop |
| 13.4 | ~2 min | lookup_in_dir, one-liner wrapping walk_directory |
| 13.5 | ~4 min | Test block with assertions |
| **Total** | **~17 min** | Single commit (steps tightly coupled — Dirent, walker, lookup all needed together for the test to mean anything) |

Easiest filesystem lesson since Lesson 10. The hard ext2 concepts (superblock, BGDT, inode table) are done; Lesson 13 is just "walk a linked list inside a data block."

**Sidebars during Lesson 13:**

- **What is lost+found?** Standard mkfs-created directory (inode 11) where fsck puts orphaned files — files whose inode survived but no directory entry points at them after a crash. Pre-created with extra empty space so fsck can dump recovered files without needing to allocate space in a possibly-corrupted filesystem.
- **Why root's `..` points at itself.** Conventional: every directory has `..`, but root has no parent. Pointing `..` at root itself eliminates the "what if no parent?" special case in path resolution. Same trick as circular sentinel nodes in linked lists. `cd ..` at the shell prompt stays at `/` because of this.
- **How inode lookup actually works.** User asked "how does one find an inode in general — do you scan the filesystem?" Two-layer answer: (1) inode NUMBER → inode struct is O(1) arithmetic (byte offset = bg_inode_table × block_size + (N-1) × inode_size); (2) PATH → inode number is a tree walk through directories, doing a linear scan within each directory's entries. Linux optimizes with the dentry cache (recent lookups) and htree indexing (large directories).
- **Tooltip clipping bug.** Lesson 13's byte-layout had tooltips that wouldn't show — root cause was `overflow: hidden` on `.byte-bar` clipping the tooltip to within the bar. Fixed by removing overflow:hidden (added explicit border-radius on first/last child cells instead). Also switched from `title=` to `data-tip=` so the native browser tooltip (500ms+ delay) doesn't fire alongside the instant CSS one.

See [[0014-lesson-12-complete]] for the prior lesson record.
