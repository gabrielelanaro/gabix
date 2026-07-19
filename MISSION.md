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
- [x] ext2 mounted as the root filesystem, replacing `ROOT_INODE` lazy_static — Lesson 14

### ELF execution
- [x] ELF header parser (magic, class, endianness, program headers) — Lesson 15
- [x] PT_LOAD segments mapped into a fresh `AddressSpace` — Lesson 16
- [x] `exec(path)` syscall — load ELF from filesystem, replace current process image — Lesson 16
- [x] A user program compiled externally, written to the disk image, loaded and executed by the kernel — Lesson 16

---

## Beyond the mission — future phases

The original mission (plug in a disk, load an ELF, execute it) is complete as of 2026-07-04. What follows are the natural next phases, in recommended order. Pick what excites you.

### Phase 1 — Real processes (multi-program OS)
The kernel currently runs one program then halts. This phase turns mini-os into an actual multi-process OS.
- [ ] **Process table** — `fork()`, `wait()`, real scheduler, per-process state (PID, PPID, exit code, FD table)
- [ ] **FD inheritance** — fork copies parent's file descriptor table (POSIX semantics)
- [ ] **Signal delivery** — `SIGCHLD` when a child exits, `SIGINT` on Ctrl-C
- **Why first:** every later phase benefits from multi-process. Networking wants concurrent connections. A shell needs fork+exec. Even a richer test harness wants to spawn helpers.
- **Estimated:** ~5–8 lessons.
- **Linux sources:** `kernel/fork.c`, `kernel/exit.c`, `kernel/sched/`

### Phase 2 — Networking (parallel of the disk path)
Same shape as Lessons 5–14 but for the network: discover device, init driver, build abstractions, send/receive. Planned in detail: [lessons/roadmap-networking.html](lessons/roadmap-networking.html). Decision made 2026-07-05: UDP is the L4 endpoint; TCP is a post-phase extension arc.
- [x] L19 — shape of the network stack (concept) + PCI find virtio-net (device `0x1041`)
- [x] L20 — virtio-net init: modern transport, MAC from device config, RX/TX virtqueues, pre-posted RX buffers
- [x] L21 — first frames: Ethernet header parse/build, `send_frame`/`recv_frame` abstraction
- [x] L22 — ARP: resolve gateway 10.0.2.2, answer requests for 10.0.2.15
- [x] L23 — IPv4: header parse/build, RFC 1071 internet checksum, protocol demux
- [x] L24 — ICMP echo: ping the gateway and print the reply — the network "hello world" (keystone)
- [x] L25 — UDP: header + pseudo-header checksum, in-kernel echo via QEMU `hostfwd`
- [ ] L26 — sockets: `socket`/`bind`/`sendto`/`recvfrom` syscalls, user-space `echo_server.elf` (objective)
- **Why second:** big satisfying parallel to the disk path. Lands a second hello-world (ping reply) plus an objective the host can talk to (`nc -u localhost 5555`). Lots of conceptual transfer from existing virtio work.
- **Environment constraint:** QEMU slirp networking — guest 10.0.2.15, gateway 10.0.2.2, inbound only via `hostfwd`, so the ping milestone is guest-initiated.
- **Estimated:** 8 lessons.
- **Linux sources:** `drivers/net/virtio_net.c`, `net/ethernet/eth.c`, `net/ipv4/{arp,ip_input,icmp,udp}.c`, `net/socket.c`

### Phase 2b — Interrupt-driven I/O (kill the poller)
All device I/O currently polls: virtio-blk busy-waits 200k iterations per `read_sector`, virtio-net has a cooperative polling worker that spin-yields forever, ARP resolution busy-polls. This phase replaces polling with hardware interrupts via MSI-X + Local APIC.
- [ ] L28 — Local APIC enable, MSI-X vector for virtio-blk, IDT ISR, replace `read_sector` poll loop with `hlt`-based wait
- [ ] L29 — virtio-net RX/TX MSI-X vectors, ISR-driven frame processing, delete `network_worker` task, blocking `recvfrom` and `poll` wakeups triggered by ISR, lost-wakeup race against hardware interrupts
- **Why now:** the polling was tolerable for development. With sockets and `poll` working, the inefficiency is the main remaining quality gap in the I/O path.
- **Estimated:** 2 lessons.
- **Linux sources:** `drivers/virtio/virtio_pci_common.c` (MSI-X setup), `arch/x86/kernel/apic/`, `kernel/irq/`

### Phase 3 — Page cache
Internal cleanup that makes everything faster and unblocks mmap.
- [ ] Frame-backed inode data (currently heap-backed via `Vec<u8>`)
- [ ] `(inode, offset) → frame` cache shared across processes
- [ ] `mmap` syscall (file-backed memory mapping)
- **Why third:** invisible to users (no new features) but makes the existing system feel snappier and unlocks future work (shared memory, file-backed IPC).
- **Estimated:** ~3–5 lessons.
- **Linux sources:** `mm/filemap.c`, `mm/mmap.c`

### Phase 4 — Dynamic linking
Handle `ET_DYN` binaries (shared libraries + PIE).
- [ ] `PT_DYNAMIC` processing
- [ ] Load-time relocations
- [ ] `PT_INTERP` handling (load `ld.so` as interpreter)
- [ ] Auxiliary vector (`auxv`) setup
- **Why fourth:** only valuable once you have user programs that *want* to share code. Without multiple user binaries that share libc, dynamic linking is overhead with no benefit.
- **Estimated:** ~5–7 lessons.
- **Linux sources:** `fs/binfmt_elf.c` (revisited), glibc `elf/dl-load.c`

### Side quests (whenever you want a break)
- [ ] **Other binary formats** — script (`#!/usr/bin/env python`) and `binfmt_misc`-style handlers. ~1–2 lessons. Small, fun, not load-bearing.
- [ ] **Filesystem mutation** — `creat`, `mkdir`, `unlink`, `rename`. Was Lesson 4's plan; ext2 mount is RW-capable now. ~2–3 lessons.
- [ ] **Permissions and ownership** — POSIX file modes, uid/gid, ACLs (Access Control Lists). ~2 lessons. Polish only.

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
- **Lesson 15** (ELF header parser): four commits `fb36235` → `f480577`. New `src/elf.rs` module with `ElfHeader` (64-byte parse) + `ProgramHeader` (56-byte parse) + constants for ET_EXEC/EM_X86_64/PT_LOAD/PF_*. The `make-disk.sh` script cross-compiles a 4-line `x86_64-unknown-none` binary (`rustc -C relocation-model=static`) and writes it to `/hello.elf`. Kernel reads /hello.elf via VFS, parses header + walks PHT, confirms entry 0x201190 inside an r-x LOAD segment. See learning-records 0016.
- **Lesson 16** (exec): three commits `9f2049a` → `407905f`. New `src/exec.rs` module: `exec_elf(path)` chains VFS → parser → `map_segment` for each PT_LOAD → `setup_user_stack`. main.rs calls it, switches CR3 to the new `AddressSpace`, and `iretq`s to ring 3 at `e_entry=0x201190`. Two new helpers in `src/memory.rs` (`zero_frame`, `write_to_frame_at`). Verified end-to-end: kernel reads ELF from disk, maps it into a fresh AS, jumps to user mode, timer interrupts prove round-trip ring transitions work. **Mission: plug in a disk, load an ELF, execute it — done.**

## Constraints
- Rust on bare-metal x86-64 (`#![no_std]`)
- No external dependencies; uses our existing heap allocator, frame allocator, and per-process AddressSpace
- Disk: virtio-blk-pci (modern transport), raw image format, ext2 filesystem
- Follow existing mini-os conventions: Socratic learning, expand every acronym on first use, write code together where possible
- Read the Linux sources alongside: `fs/read_write.c`, `block/`, `fs/ext2/`, `drivers/block/virtio_blk.c`, `fs/exec.c`

## Timeline (2026-06-13 → 2026-07-04)

**By the numbers:**

| Metric | Value |
|---|---|
| Calendar span | 21 days |
| Active days | 11 |
| Total commits | 103 |
| Estimated focused hours | ~50h |
| Final LoC | ~3000 (src/) + lessons/tours/records |

**Sessions (commit windows from first to last commit each day):**

| Date | Commits | Hours | What landed |
|---|---:|---:|---|
| 06-13 | 10 | ~4.5h | Repo setup, concept references, VGA writer |
| 06-14 | 11 | ~9.5h | Boot/bootimage, VFS foundation (Lessons 1–2) |
| 06-20 | 9 | ~7h | Real inodes + path resolution (Lesson 3) |
| 06-21 | 6 | ~5h | User mode + iretq (M3) |
| 06-22 | 4 | ~5.5h | Preemptive scheduler (M3) |
| 06-25 | 1 | — | (docs) |
| 06-27 | 3 | ~5h | Syscall MSRs + handler (M3) |
| 06-28 | 10 | ~6h | Memory layer — AddressSpace, fork shape |
| 06-29 | 5 | ~0.5h | Quick fixes |
| 07-03 | 7 | ~0.2h | Lesson prep |
| **07-04** | **37** | **~7.5h** | **Lessons 13–16: disk → ext2 → VFS → ELF → ring 3** |

**Shape of the work:**

- **M2/M3 phase (mid-June):** heap allocator, frame allocator, paging,
  AddressSpace, GDT/IDT, PIC/PIT, user-mode entry, scheduler,
  syscall mechanism. The kernel *infrastructure*. Mostly contiguous
  work — 7 sessions in 9 days.
- **Gap (June 30 – July 2):** 5 days off.
- **Disk path (Lessons 5–16, late June + July 4):** PCI enumeration,
  virtio handshake, virtqueues, block layer, ext2 superblock, BGD,
  inode, directory walk, mount via Filesystem trait, ELF parser,
  exec. The *mission*. Most of this landed in two focused sessions
  (June 28 and July 4).
- **Final push (July 4, ~7.5h):** 37 commits. Lessons 13 (ext2 dir
  walk), 14 (mount via trait), 15 (ELF parser), 16 (exec). All four
  lessons from "no ext2 directory code" to "hello.elf calls
  sys_write from ring 3" in a single sitting.

**Pace calibration for future planning:**

- A "lesson" (defined concept → working code → tour + records) takes
  1–3 hours once the conceptual foundation has landed in conversation.
- Conceptual foundation work — the Socratic back-and-forth that
  builds the mental model — takes longer than the code itself and is
  what determines whether the lesson sticks. Lesson 15 took ~30 min
  of code but ~1h of conceptual conversation first.
- The shortest unit of meaningful work is ~30 min (one step within
  a lesson: struct definition, parser, single test). The longest
  single lesson-end-to-end was Lesson 16 at ~2.5h.


## Out of scope (for this push to disk)
- Asynchronous I/O (`aio`, `io_uring`)
- Memory-mapped files (`mmap`)
- Networking sockets
- Other disk interfaces (ATA/IDE, NVMe) — virtio-blk only
- Other filesystems (ext4, FAT, XFS) — ext2 only
