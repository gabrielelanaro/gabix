# Lesson 3 complete — real inodes, byte-format directories, path resolution

User completed Lesson 3 (all six sub-steps 3.1 through 3.6) in a single
session. End-to-end test passes: user program opens two files at two
different paths, reads both, prints both contents to serial. The kernel
parses paths, walks a directory, looks up inodes by number from a global
registry, wraps them in `File` objects.

**What was built (across 6 commits, one per step):**

| Step | Commit | What landed |
|---|---|---|
| 3.1 | `3b743cc` | `InodeKind` enum, `ino`/`kind` fields on `Inode`, `REGISTRY` (BTreeMap), `NEXT_INO` atomic counter, `new_file`/`new_dir`/`lookup` constructors |
| 3.2 | `8307fd3` | `encode_dirent`/`lookup_in_dir` helpers; format `[name_len: u8][name: N][ino: u64 LE]`. Boot test for encode/decode |
| 3.3 | `9917e71` | `ROOT_INODE` lazy_static with reserved `ROOT_INO=1` and `HELLO_INO=2`. `HARDCODED_INODE` repurposed as legacy alias |
| 3.4 | `b4e99d7` | `resolve_path` (six-step walk), `read_user_path` with NULL guard, `sys_open` does real path resolution |
| 3.5 | `394e749` | `USER_PROGRAM` updated to pass `"/hello.txt"` — end-to-end chain restored |
| 3.6 | `c4f6101` | Second file `secret.txt` (`SECRET_INO=3`), `USER_PROGRAM` doubled to two cycles, kernel-internal multi-file test |

**Zone of proximal development now:**
- Directory byte format and dirent encode/decode absorbed via the worked-example hexdump in the design chat
- Inode numbers vs pointers as a *persistence* decision (not just performance) absorbed
- Path resolution algorithm and the O(1) skip via length-prefix absorbed
- `lazy_static` initialization order (HARDCODED_INODE forces ROOT_INODE init via `let _ = &*ROOT_INODE`) absorbed
- Per-snippet filename captions in tours established as convention
- Ready for Lesson 4: writing and creating files (`creat`, `mkdir`, runtime mutation of the directory's bytes)

**Calibration data — Lesson 3 total wall clock:**

| Step | Time | Notes |
|---|---|---|
| 3.1 | ~110s | read fs.rs + edits + build + QEMU |
| 3.2 | ~160s | includes a Vec-import fix cycle |
| 3.3 | ~88s | fastest — compose-the-pieces step |
| 3.4 | ~630s | longest — most time was reading syscall.rs and thinking through shippability of 3.4 alone (NULL guard in `read_user_path` was the unlock) |
| 3.5 | ~100s | byte-level USER_PROGRAM rewrite |
| 3.6 | ~220s | two-cycle USER_PROGRAM + second file in root |
| **Total** | **~22 min** | Six focused commits |

**Pattern confirmed:** estimates for small well-scoped tasks in familiar codebase
continue to run 3–5x too high. The one outlier (3.4) was long because of
*design thinking* (shippability concern), not code volume. Code work itself
stayed in the 1–4 minute range across all six steps.

**Sidebars during Lesson 3 worth tracking:**
- Why length-prefix beats null-termination (O(1) skip-to-next-entry)
- Why inode numbers vs pointers (persistence across reboot, eviction)
- xarray (Linux's main index-to-pointer data structure; the registry's analog)
- `dcache` / htree for directory-lookup performance at scale
- Why all real filesystems have inode *kinds* (load-bearing in VFS dispatch, not a departure)

**Teaching workflow developments during Lesson 3:**
- Tour convention established: each lesson has a companion `tours/tour-<NNNN>-<dash-name>.html` showing the actual Rust that landed
- Per-snippet filename caption requirement recorded after user pushed back ("otherwise I lose track")
- "Don't refer to step numbers without saying what the step does" recorded as a communication preference
- Step-number-anonymous reference style now standard

See [[0003-memory-layering]], [[0004-allocator-routing]], [[0005-user-pointer-validation]], [[0006-lesson-2-complete]] for prior sidebar learnings.
