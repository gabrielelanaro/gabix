# VFS Glossary

Vocabulary for the mini-os VFS (Virtual File System) work. Terms get added as the user
demonstrates understanding — this is a record of compressed knowledge, not a dictionary
to learn from.

A visual review version of this glossary lives at
[reference/vfs-glossary.html](reference/vfs-glossary.html).

## Core VFS abstractions

**VFS (Virtual File System)**:
The kernel layer between user syscalls (`open`, `read`, `write`, `close`) and
filesystem-specific code (ext4, tmpfs, procfs). Dispatches each syscall to the right
backend via function-pointer tables on the file and inode objects.
_Avoid_: filesystem (a VFS is the dispatch layer; a filesystem is what gets dispatched to)

**Superblock**:
A mounted filesystem instance. Holds global info (block size, free space, root inode
pointer) and the operations table for this filesystem. One per mount.

**Inode (Index Node)**:
A file's identity. Holds metadata (size, owner uid, group gid, permissions, timestamps,
link count, allocated block count) and a way to fetch the data. Has no name; multiple
dentries can point at one inode.
_Avoid_: file (a file is what an inode represents; the inode is the metadata object)

**Dentry (Directory Entry)**:
A name → inode link. Lives in the parent directory. Forms the directory tree. Multiple
dentries can point at the same inode (hard links).
_Avoid_: filename (filename is the user-visible string; dentry is the kernel object)

**File (struct file)**:
An open instance of an inode. Holds the cursor (offset), access mode (read/write), and a
reference to the underlying inode. Private per `open()` call; shared across `fork()` and
`dup()`.

**File Descriptor (FD)**:
A per-process integer index into the FD table. Maps to a `struct file`. Always
non-negative; 0/1/2 are reserved by convention for stdin/stdout/stderr.

## Lifecycle fields

**Link Count (i_nlink)**:
The number of dentries pointing at an inode. Persisted on disk. When it reaches 0, the
inode's data blocks can be freed (subject to the reference count also being 0).

**Reference Count (i_count)**:
The number of open `struct file` instances pointing at an inode. In-memory only (not
persisted). When it reaches 0, the kernel can drop the in-memory inode (subject to the
link count also being 0 for freeing data blocks).

**Hard Link**:
A second dentry pointing at an existing inode. Created by `ln source target`. Both names
refer to the same data; the inode's link count goes up by one. Deleting one name does
not delete the data; only when the last dentry is removed does the inode become a
candidate for freeing.

**atime / mtime / ctime**:
The three timestamps in an inode. `atime` = access time (updated on read). `mtime` =
modify time (updated on write to the data). `ctime` = change time (updated on any inode
metadata change — chmod, chown, rename).

## FD-table operations

**dup**:
Duplicate an FD. Returns a new FD number pointing at the same `struct file` as the
original. Both FDs share the cursor and access mode. `dup2(old, new)` lets you specify
the target FD number — used by shells for redirection (`>`, `2>&1`) and pipelines (`|`).
_Avoid_: copy (the FD number is new; the underlying File is shared, not copied)

## Memory layering

**Frame allocator**:
The lowest-level allocator — manages all of physical RAM as 4 KB frames.
Returns frame numbers (not pointers); caller maps the frame to a virtual
address to use it. mini-os's is a bitmap; Linux's analog is the buddy page
allocator.

**Heap (kernel heap)**:
Sub-page allocator layered on top of the frame allocator. mini-os's is
`linked_list_allocator` wrapped in `#[global_allocator]`, managing a
fixed 100 KB region (which is itself backed by 25 frames allocated from
the frame allocator). Linux's analog is the slab allocator (`kmalloc`).
_Avoid_: "the heap" as a synonym for "memory" — heap is one consumer of
frames, not a separate pool.

**`#[global_allocator]`**:
Rust attribute that registers a static as the allocator for all `Box`,
`Vec`, `String`, `Arc`, etc. in the crate. In mini-os this is
`ALLOCATOR: LockedHeap` in `src/allocator.rs`. Without this attribute
and an `init(start, size)` call, `Box::new` panics.

**Direct map (physical_memory_offset)**:
A virtual address range where every byte of physical RAM is reachable
linearly: `virt_addr = direct_map_base + phys_addr`. mini-os's
`PHYS_OFFSET` (set from `boot_info.physical_memory_offset`, currently
`0x180_0000_0000`) is the direct-map base. Used by `phys_to_virt()` to
reach physical frames (e.g., page tables) from kernel code without
setting up a translation.

**Page cache**:
In Linux, the kernel's cache of recently-accessed file pages in RAM.
Indexed by `(inode, page_offset)` in an `address_space` per inode. For
ramfs, the page cache IS the storage (no disk backup). For disk-backed
filesystems, the page cache is a window over the disk — pages get loaded
on read miss, flushed on writeback.
_Avoid_: "the page cache is the heap" — different allocators. Page cache
uses the buddy page allocator; heap (slab) is for small kernel objects.

**`address_space`** (pending — mentioned but not deeply explored):
Linux's per-inode structure that holds the page cache tree. Roughly
`HashMap<page_offset, struct page>` plus operations for the filesystem
to populate/evict pages.

**MMIO (Memory-Mapped I/O)**:
Accessing device registers via normal memory load/store instructions,
where the address routes to a device's BAR (Base Address Register)
instead of RAM. The CPU's `mov` doesn't know whether an address hits
RAM or a device — only the page tables and the PCI host bridge know.
Requires page-table entries with the `NO_CACHE` flag so the CPU doesn't
return stale cached values. The foundation of every modern PCIe device
driver: network cards, GPUs (Graphics Processing Units), NVMe (Non-
Volatile Memory Express) drives, USB (Universal Serial Bus)
controllers, virtio devices.

**Port I/O (`in` / `out` instructions)**:
x86's OTHER address space, separate from the memory address space.
Accessed via the `in` and `out` CPU instructions (not `mov`). 16-bit
address range (64K ports). Legacy. PCI config space lives here at
ports 0xCF8 (address) and 0xCFC (data). Most modern devices use MMIO
instead; I/O ports survive for backward compatibility and PCI config
access. ARM, for comparison, never had this split — everything is
memory-mapped there.

**Mapping**:
A single virtual-address → physical-address entry in a page table.
"Virtual `0x180FE000000` maps to physical `0xFE000000`" is one
mapping. An entire address space is a collection of mappings. The
direct map is a *regular pattern* of mappings — every entry follows
`VA = PA + offset`, which lets software compute translations without
walking the page table.

**Address space (two meanings)**:
Overloaded term. (1) *The* address space — the theoretical range of
possible addresses. "The x86-64 virtual address space is 48-bit
canonical, 256 TB." Property of the architecture. (2) *An* address
space — a particular set of mappings, one full page-table hierarchy.
"The kernel's address space," "the user program's address space."
Property of a process or context. Different address spaces can map
the same VA to different PAs — that's how fork works.

**Volatile access (`read_volatile` / `write_volatile`)**:
Rust intrinsics (`core::ptr::read_volatile`, `write_volatile`) that
prevent the compiler from eliding, reordering, or caching memory
accesses. Required for MMIO correctness. Without `read_volatile`, the
compiler might optimize away a device-status poll, assuming the value
doesn't change between reads. Each call compiles to exactly one load
or store at the specified address — no batching, no caching.

**`NO_CACHE` (PCD) page-table flag**:
When set on a page-table entry, the CPU bypasses L1/L2 cache for
loads/stores from that page. Required for MMIO: device registers
change without CPU involvement (status bits get set, completions
arrive), and cached reads would return stale values, breaking the
device protocol. The x86 manual name is PCD (Page Cache Disable).
Worked example: virtio's `device_status` byte changes when the device
accepts our features — without NO_CACHE, we'd read the cached "before
FEATURES_OK" value and the handshake would never see acceptance.

**IOMMU (Input/Output Memory Management Unit)**:
The device-side analog of the CPU's MMU (Memory Management Unit).
Where the CPU's MMU translates virtual addresses to physical for code
running on the CPU, the IOMMU translates addresses for devices doing
DMA (Direct Memory Access). Without one, a device's DMA reaches
physical RAM directly at whatever address the driver gave it — the
device can touch anything. With one, every DMA address passes through
a translation table (analogous to a page table) that the kernel
pre-programs with the device's allowed pages; anything outside the
allow-list is rejected. The classic use case is PCI passthrough:
handing a real PCI device to a virtual machine would be a security
hole without an IOMMU, because the VM could DMA into host memory and
read other VMs. mini-os has no IOMMU (QEMU's virtio devices are
trusted); real Linux on real hardware always has one. Drivers
typically use a DMA API (`dma_map` / `dma_unmap`) that abstracts
whether an IOMMU is present.

## Syscall / user-kernel boundary

**`USER_ADDR_LIMIT`**:
Constant in mini-os (`0x0000_8000_0000_0000`, 2^47 — first non-canonical
address). Any pointer at or above this is in the kernel upper half or
the non-canonical hole. Used by `is_user_range()` to validate syscall
arguments.

**`is_user_range(ptr, len)`**:
mini-os helper that returns true if `[ptr, ptr+len)` is entirely within
user space. Used by every syscall taking a user pointer (`sys_read`,
`sys_write`) to reject kernel-pointer attacks. Returns `u64::MAX`
(EFAULT sentinel) on failure. Does NOT catch in-range-but-unmapped
pointers — that requires a page-fault handler we don't have.

## Not yet promoted (user exposed, not demonstrated)

- superblock operations table (`sb_ops`)
- file operations table (`fops`) — the function-pointer table on File/Inode that VFS dispatches through
- dentry cache (dcache) — in-memory cache of resolved paths
- ramfs — the simplest Linux filesystem, all in memory, no disk
- `address_space` — see Memory layering section above (marked pending there too)
- buddy allocator — Linux's analog of our frame allocator; allocates 2^order pages
- slab allocator — Linux's analog of our heap; sub-page allocs via object pools
- `phys_to_virt()` — mini-os helper that adds PHYS_OFFSET to a physical address
- `ioctl` — Input/Output Control syscall for device-specific operations beyond read/write
- TTY (TeleTYpewriter) — terminal device abstraction
- `ptrace` — process-trace primitive used by debuggers; can rewrite FDs of a running process
- `TIOCSTI` — Terminal I/O Control Simulate Terminal Input; pushes bytes into TTY input queue
