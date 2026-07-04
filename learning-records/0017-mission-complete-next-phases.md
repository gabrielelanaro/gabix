# Mission complete — next phases brainstormed

The 4-month roadmap's mission statement ("plug in a disk, load an ELF
binary from it, and execute it") landed today, 2026-07-04, when
`hello.elf` running in ring 3 called `sys_write` and the kernel wrote
"hello, world" to the serial port. Total elapsed: 21 calendar days,
11 active sessions, ~50 focused hours, 103 commits. See the Timeline
section in MISSION.md for the per-session breakdown.

**What "complete" actually means here:**

A real Rust source file (`scripts/make-disk.sh`'s heredoc) compiled
outside the kernel with `rustc --target x86_64-unknown-none`, written
into a disk image via `debugfs`, then loaded by the running kernel
through this chain:

  disk → virtio-blk (PCI + modern transport) → block layer
       → ext2 superblock/BGDT/inode/directory walk
       → VFS Filesystem trait + REGISTRY cache
       → ELF parser (Lesson 15)
       → ELF loader (Lesson 16: fresh AddressSpace, map PT_LOADs,
         user stack, switch CR3, iretq)
       → ring 3 → `syscall` instruction → kernel handler → serial

Every layer was built from scratch over the project's lifetime. None
of it pre-existed.

**Next-phases brainstorm:**

After landing, the user brainstormed five directions for what's next:

1. **Process table** — fork, wait, real scheduler, per-process state.
   Kernel currently runs one program then halts.
2. **Networking** — exact parallel of the disk path for virtio-net.
   Lands a second "hello world" (ping response or TCP echo).
3. **Page cache** — currently heap-backed `Vec<u8>` per inode; a real
   page cache shares file-backed frames by `(inode, offset)`.
4. **Dynamic linking** — handle `ET_DYN` (`.so` + PIE), load-time
   relocations, `PT_INTERP` interpreter (`ld.so`), auxiliary vector.
5. **Other binary formats** — script (`#!`) and `binfmt_misc`-style
   dispatch. Small side quest.

Plus existing deferred polish items folded into the roadmap:
filesystem mutation (`creat`/`mkdir`/`unlink`), POSIX permissions,
ACLs.

**Recommended ordering** (recorded in MISSION.md as the new
"Beyond the mission — future phases" section):

  Phase 1: Process table  →  foundation for everything interactive
  Phase 2: Networking      →  big satisfying parallel, 2nd hello world
  Phase 3: Page cache      →  internal cleanup, unblocks mmap
  Phase 4: Dynamic linking →  needs multiple user binaries first
  Side quests              →  filesystem mutation, permissions, #!

The user has not yet committed to which phase to start next. The
MISSION.md section is written so they can pick what excites them.

**Reflection on what landed well in this session:**

The mission-end session was 7.5 hours and 37 commits, but the
critical insight was conceptual not code-shaped. The user
self-reported at the start: "I'm not sure really how an ELF file is
structured." A 30-minute Socratic exchange about *why* a binary file
is more than raw bytes (code can't be relocated because addresses
are baked into the instruction bytes themselves; data can) unlocked
the rest. The code itself — `ElfHeader::parse`, `ProgramHeader`,
`map_segment`, `exec_elf`, `setup_user_stack`, `enter_user_at` — was
mechanical once the model landed. The lesson plan already reflected
this in the byte-layout details and the anatomy walkthrough, but
the conversation that actually taught the user happened in chat,
not in the lesson HTML.

**Calibration for future sessions:**

- Pace: ~50 focused hours / 16 lessons = ~3h per lesson
  (including conceptual conversation, code, tour, learning record).
- Conceptual conversation is the long pole, not the code. Worth
  protecting time for it explicitly.
- The user is now comfortable enough with the storage + execution
  stack to drive next phases with less Socratic hand-holding.
  Their `user_learning_style.md` memory should evolve toward
  "just-do-it with explanations" as the default for familiar
  territory (ext2-shaped things, virtio-shaped things, paging-
  shaped things), with Socratic mode reserved for genuinely new
  conceptual areas (TCP state machine, ARP, scheduling algorithms).
- "Hello world" is a powerful milestone shape. Disk path: read
  hello.txt. ELF path: print hello, world from ring 3. Network
  path: respond to ping. Each phase should have one.

See [[0016-lesson-15-complete]] for the prior lesson record.
