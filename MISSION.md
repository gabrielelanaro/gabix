# Mission: Reach a disk-backed filesystem and ELF execution

## Why
Build a Virtual File System layer in the mini-os bare-metal kernel as a means
to a concrete goal: **plug in a disk, load an ELF (Executable and Linkable
Format) binary from it, and execute it.** The VFS work is the generic file
interface that a disk-backed filesystem plugs into; disk is what makes the
filesystem real instead of a boot-time snapshot.

The deeper goal is to learn Linux's storage stack internals by re-implementing
them: PCI (Peripheral Component Interconnect) device discovery, virtio,
block layers, ext2, page cache, ELF loading, `exec()`. Each layer in the
kernel has a Linux counterpart we can read alongside.

## Success looks like

### VFS foundation (done — Lessons 1–3)
- [x] A user program can call `open(...)` and get back a file descriptor — Lesson 2
- [x] `read(fd, buf, len)` returns the file's bytes — Lesson 2
- [x] `write(fd, buf, len)` modifies the file (or prints to stdout for fd=1) — Lesson 2
- [x] `close(fd)` releases the descriptor — Lesson 2
- [x] Each process has its own file descriptor table — Lesson 2
- [x] Path resolution walks the directory tree — Lesson 3
- [x] Multiple files: real directory with byte-format dirents — Lesson 3

### Disk path (current focus)
- [x] QEMU launched with a raw disk image attached via virtio-blk-pci — Lesson 5
- [x] PCI bus enumeration from the kernel — list every device, read vendor/device IDs — Lesson 5
- [x] Find the virtio-blk device on the PCI bus (vendor `0x1AF4`) — Lesson 5
- [x] virtio PCI modern initialization — probe, negotiate features, enable — Lesson 6
- [x] Virtqueues set up (allocation, configuration, free list, submit, kick, poll) — Lesson 7
- [x] Virtqueues set up — read/write requests submitted and completed — Lessons 7-8
- [x] First successful read of sector 0 (boot signature / known bytes) — Lesson 8 (keystone 1)
- [x] Block layer abstraction — `read_sector(n) -> [u8; 512]` — Lesson 9

### ext2 filesystem
- [x] Superblock parsed (offset 1024, fields validated) — Lesson 10
- [x] Block group descriptor table walked — Lesson 12
- [x] Inode lookup by number (root inode = 2 by ext2 convention) — Lesson 12
- [x] ext2 directory entries parsed — different byte format from our VFS dirents — Lesson 13
- [ ] ext2 mounted as the root filesystem, replacing `ROOT_INODE` lazy_static

### ELF execution
- [ ] ELF header parser (magic, class, endianness, program headers)
- [ ] PT_LOAD segments mapped into a fresh `AddressSpace`
- [ ] `exec(path)` syscall — load ELF from filesystem, replace current process image
- [ ] A user program compiled externally, written to the disk image, loaded and executed by the kernel

### Polish (deferred — not on critical path)
- [ ] `creat`, `mkdir`, `unlink` for runtime filesystem mutation (was Lesson 4 plan)
- [ ] Frame-backed inode data instead of heap-backed (true page-cache shape)
- [ ] Fork inherits parent's file descriptor table (POSIX semantics)
- [ ] Permissions, ownership, ACLs (Access Control Lists)
- [ ] Reading Linux's `fs/read_write.c`, `block/`, `fs/ext2/`, `fs/exec.c` and the patterns making sense

## Progress
- **Lesson 1** (concept foundation): four abstractions absorbed. See learning-records 0001.
- **Lesson 2** (VFS foundation built): committed as `72c215f`. open/read/write/close working end-to-end on hardcoded inode. See learning-records 0006.
- **Lesson 3** (real inodes and paths built): six steps across commits `3b743cc` → `c4f6101`. Filesystem has a real root directory with two files, path resolution walks the tree, USER_PROGRAM resolves paths end-to-end. See learning-records 0007.
- **Lesson 5** (PCI bus enumeration): four steps across commits `7ec8c03` → `e05b0de`. Kernel walks bus 0, lists every device, identifies virtio-blk specifically, decodes its BAR layout. See learning-records 0008.
- **Lesson 6** (virtio device initialization): four steps across commits `fc21746` → `d5f03d3`. Kernel parses the PCI capability list, maps the MMIO regions, walks through the full virtio handshake (reset → ACKNOWLEDGE → DRIVER → feature negotiation → FEATURES_OK → DRIVER_OK). Device is now live. See learning-records 0009.
- **Lesson 7** (virtqueues): five steps across commits `49a9f79` → `a5ec3f7`. Kernel allocates the three regions (descriptor table, available ring, used ring) as page-aligned frames, configures the queue in common config, builds a descriptor free list, implements `submit_chain` with Release ordering, and adds `kick` + `poll_completion` (with Acquire). Queue is active; mechanics verified end-to-end. See learning-records 0010.
- **Lesson 8** (first disk read — Keystone 1): five steps across commits `233f4dd` → `d42e82a`. Built a 3-descriptor virtio-blk read chain (header + data + status), submitted, kicked, polled, verified bytes. The kernel reads sector 0 and confirms `MINIOS_DISK_v0\n` + 497 trailing zeros. Bytes flow from disk for the first time. See learning-records 0011.
- **Lesson 9** (block layer abstraction): single commit `6875d9b` (refactor + new method + test form one cohesive change). VirtioBlk now owns the VirtQueue; `setup_queue(64)` allocates and configures in one call. New `read_sector(sector) -> [u8; 512]` method hides the full descriptor-chain + DMA + submit/kick/poll dance behind one synchronous call. ext2 can mount on top. See learning-records 0012.
- **Lesson 10** (ext2 superblock parser): four commits `f7cc83e` (mkfs.ext2 the disk) → `34a7e78` (verify magic + fields). New `src/ext2.rs` module with `Superblock` struct + parser using `from_le_bytes`. Reads sectors 2 and 3, concatenates 1024 bytes, parses. Magic = 0xEF53, block_size = 1024, 128 inodes, 1024 blocks. Filesystem phase begins. See learning-records 0013.
- **Lesson 12** (ext2 inode lookup): two commits `ba06f20` (superblock extras) → `571ebba` (BGD + Inode + read_inode). Extended Superblock with `inodes_per_group` and `inode_size`. Added `BlockGroupDescriptor` (32 bytes), `Inode` (128 bytes parsed), `read_block_group_descriptor`, `read_inode`. Verified inode 2 is the root directory at block 40 with mode 0x41ED. See learning-records 0014.
- **Lesson 13** (ext2 directory walk): single commit `2fdc4cc` (Dirent + read_block + walk_directory + lookup_in_dir). Variable-length dirent parser, linked-list traversal via rec_len. Root directory contents: `.`, `..`, `lost+found` all verified. `lookup_in_dir` finds lost+found → inode 11, returns None for missing names. See learning-records 0015.

## Constraints
- Rust on bare-metal x86-64 (`#![no_std]`)
- No external dependencies; uses our existing heap allocator, frame allocator, and per-process AddressSpace
- Disk: virtio-blk-pci (modern transport), raw image format, ext2 filesystem
- Follow existing mini-os conventions: Socratic learning, expand every acronym on first use, write code together where possible
- Read the Linux sources alongside: `fs/read_write.c`, `block/`, `fs/ext2/`, `drivers/block/virtio_blk.c`, `fs/exec.c`

## Out of scope (for this push to disk)
- Asynchronous I/O (`aio`, `io_uring`)
- Memory-mapped files (`mmap`)
- Networking sockets
- Other disk interfaces (ATA/IDE, NVMe) — virtio-blk only
- Other filesystems (ext4, FAT, XFS) — ext2 only
