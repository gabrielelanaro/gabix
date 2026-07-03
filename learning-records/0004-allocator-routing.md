# Vec and Box route through the same #[global_allocator]

User correctly identified (during Lesson 2 sidebar) that switching an
inode's data from `Vec<u8>` to `BTreeMap<u64, Box<[u8; 4096]>>` doesn't
actually help for ramfs at scale. Both pull from the same 100 KB heap —
`Box` is just a fixed-size chunk vs `Vec`'s growable buffer, same
allocator underneath.

To truly match Linux's page-cache pattern (file data in frames, not
heap), we'd need to **bypass the heap entirely** and call
`frame_allocator::allocate()` directly, storing the resulting frame
numbers in the inode. Linux does this via `alloc_pages()` / the buddy
page allocator; the page cache is `address_space` indexing `(inode, page_offset) → struct page`.

**Decision made:** defer the frame-backed inode refactor. User chose
"stay with `Vec<u8>` for Lesson 2" because:
- Test files are tiny (10 bytes — `"hello vfs\n"`)
- The frame-backed version is ~2-3 hours of work
- Real forcing function is ext2 (roadmap 4.7), where the same page-cache
  machinery backs both ramfs and ext2

**Implications for future sessions:**
- Don't re-pitch `Box<[u8; 4096]>` as a "more Linux-like" inode shape — it isn't, and the user knows it
- If user asks about ramfs again, the frame-backed version is the actual answer, not Box
- When ext2 milestone approaches, this refactor becomes the right time to swap in frames

See [[0003-memory-layering]] for the foundational layering the user absorbed first.
