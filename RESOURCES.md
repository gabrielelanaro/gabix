# VFS Resources

## Knowledge

### Primary sources (Linux kernel)

- [Documentation/filesystems/vfs.rst — The Linux VFS Documentation](https://www.kernel.org/doc/html/latest/filesystems/vfs.html)
  The authoritative reference. Use for: the four VFS objects (superblock, inode, dentry, file), the method tables each one exposes, the syscall-to-filesystem dispatch path.

- [Linux source: fs/read_write.c](https://elixir.bootlin.com/linux/latest/source/fs/read_write.c)
  Implementation of `read`, `write`, `lseek`, `pread`, `pwrite` syscalls. Use for: seeing how a syscall walks from FD (File Descriptor) → `struct file` → `file_operations` → filesystem-specific handler.

- [Linux source: fs/ramfs/](https://elixir.bootlin.com/linux/latest/source/fs/ramfs)
  The simplest filesystem in the kernel (everything in memory, no disk). Use for: a minimal example of `inode_operations` and `file_operations`.

- [Linux source: include/linux/fs.h](https://elixir.bootlin.com/linux/latest/source/include/linux/fs.h)
  Definitions of `struct file`, `struct inode`, `struct file_operations`, etc. Use for: cross-referencing what each field means.

### Books

- [_Linux Kernel Development_ by Robert Love, Chapter 12: The Virtual Filesystem](https://rlove.org/)
  The most readable overview. Use for: intuition for the abstraction before reading kernel source.

- [_Understanding the Linux Kernel_ by Bovet & Cesati, Chapter 12: The Virtual Filesystem](https://www.oreilly.com/library/view/understanding-the-linux/0596005653/)
  Heavier than Love; useful for the locking and lifecycle details once we have the basics.

### Companion Rust kernels

- [Theseus OS — kernel/fs/](https://github.com/theseus-os/Theseus/tree/theseus_main/kernel/fs)
  Rust OS with a conventional VFS layer. Use for: idiomatic Rust patterns for these abstractions.

- [Redox OS — scheme/](https://doc.redox-os.org/book/ch046-schemes.html)
  Different design (unified resource scheme, not POSIX VFS), but useful for the comparison and the "what would a from-scratch design look like in Rust?" question.

## Wisdom (Communities)

- [OSDev forum — File Systems Design](https://forum.osdev.org/viewforum.php?f=1)
  Decades of discussion on VFS implementations for hobby OSes. Strong opinions, often well-reasoned.

- [r/osdev](https://reddit.com/r/osdev)
  Active subreddit; good for "is this design reasonable?" sanity checks.

## Gaps

- No single "VFS design from scratch" reference exists for a Rust no_std context. The Robert Love chapter plus Theseus source is the closest combination. We will be filling this gap with our own lessons.
