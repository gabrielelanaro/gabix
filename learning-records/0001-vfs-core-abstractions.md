# VFS Core Abstractions — Inode, Dentry, File, FD understood

User demonstrated solid understanding of the four VFS abstractions and their lifetimes by
correctly answering probes after Lesson 1. Specifically:

- **Inode metadata** — correctly identified that almost everything `ls -l` shows (mode, link
  count, owner uid, group gid, size, timestamps) lives in the inode; the filename does not
  (it lives in the dentry).
- **Link count vs reference count** — correctly identified that `rm` of one hard link leaves
  the inode alive while others point at it; correctly predicted disk-space exhaustion when a
  deleted-but-open file is held by a long-running writer.
- **Cursor sharing** — correctly identified that two `open()` calls produce independent
  cursors (cursor lives in `struct file`), and absorbed the counter-case that `fork()` and
  `dup()` share a single `struct file` and therefore share a cursor.

**Implications for future sessions:** The inode/dentry/file/FD split is now baseline
vocabulary. Future lessons can refer to these four objects without re-explaining. The
link-count / reference-count distinction will matter concretely when we design mini-os's
inode struct (need both fields) and our `sys_close` (decrements refcount, not linkcount).
