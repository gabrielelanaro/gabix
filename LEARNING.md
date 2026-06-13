# Learning Plan: mini-os

Goal: learn Linux internals by re-implementing a Linux-like bare-metal OS in Rust.

Approach: follow the "Writing an OS in Rust" tutorial (phil-opp.com) as the spine, and at each milestone read the corresponding Linux source so the abstractions we build connect to real Linux.

## Roadmap

Status legend: `[ ]` not started · `[~]` in progress · `[x]` done

### Month 1 — Skeleton & boot
- [ ] 1.1 Freestanding Rust binary, `no_std`, custom panic handler
- [ ] 1.2 `bootimage` / `bootloader` crate setup, boot in QEMU
- [ ] 1.3 VGA text-mode buffer, print "Hello, World"
- [ ] 1.4 GDT (Global Descriptor Table)
- [ ] 1.5 IDT + interrupt handlers
- [ ] 1.6 Hardware interrupts: PIC/APIC timer
- [ ] 1.7 Panic with stack trace

**Linux to read alongside:** `init/main.c`, `arch/x86/kernel/`, `arch/x86/entry/`

### Month 2 — Memory management
- [ ] 2.1 Frame allocator (bitmap or buddy-lite)
- [ ] 2.2 4-level paging, map kernel higher-half
- [ ] 2.3 Heap allocator (`linked_list_allocator` then roll your own)
- [ ] 2.4 `Box` / `Vec` / `String` working in kernel
- [ ] 2.5 `mapper` API for arbitrary virtual→physical mapping

**Linux to read alongside:** `mm/page_alloc.c` (buddy allocator), `mm/memory.c`, `include/linux/mm.h`

### Month 3 — Processes & scheduling
- [ ] 3.1 `Task` struct (context, kernel stack, state)
- [ ] 3.2 Context switch routine (save/restore registers)
- [ ] 3.3 Cooperative scheduler round-robin
- [ ] 3.4 Preemptive scheduling via timer interrupt
- [ ] 3.5 Kernel threads (idle task, demo workers)
- [ ] 3.6 User/kernel mode ring transition (TSS, `syscall`/`sysret`)

**Linux to read alongside:** `include/linux/sched.h` (`task_struct`), `kernel/sched/fair.c` (CFS), `arch/x86/entry/entry_64.S`

### Month 4 — Syscalls, VFS, IPC
- [ ] 4.1 Syscall gate (`syscall`/`sysret` ABI), dispatch table
- [ ] 4.2 Core syscalls: `write`, `exit`, `getpid`
- [ ] 4.3 `fork` / `exec` / `wait`
- [ ] 4.4 VFS layer: `File`, `inode`, `open`/`read`/`write`/`close`
- [ ] 4.5 RamFS implementation
- [ ] 4.6 Pipes (`|`)
- [ ] 4.7 Optional: read-only ext2 driver

**Linux to read alongside:** `fs/read_write.c`, `fs/pipe.c`, `fs/ramfs/`, `include/linux/syscalls.h`, `fs/ext2/`

## Toolchain

- Nightly Rust (`rustup default nightly`)
- QEMU (`qemu-system-x86_64`)
- `bootimage` + `bootloader` crates
- `gdb` (multiarch) for kernel debugging

## Reference material

**Primary**
- Writing an OS in Rust — https://os.phil-opp.com
- OSDev wiki — https://wiki.osdev.org

**Conceptual**
- "Operating Systems: Three Easy Pieces" (free) — http://pages.cs.wisc.edu/~remzi/OSTEP/
- "Linux Kernel Development" — Robert Love

**Code to read**
- Redox OS kernel — https://gitlab.redux-os.org/redox-os/kernel
- Theseus OS — https://github.com/theseus-os/Theseus
- Linux source — https://elixir.bootlin.com/linux

## Milestones

A milestone is a "demo-able" checkpoint. Worth a commit + tag + log entry.

- **M0 — Hello, World** (end of 1.3): boots in QEMU, prints to screen.
- **M1 — Interrupts alive** (end of Month 1): timer ticks, panic stack trace.
- **M2 — Heap working** (end of Month 2): can `Box::new` in kernel.
- **M3 — Scheduling** (end of Month 3): two tasks printing concurrently.
- **M4 — Userspace syscalls** (end of 4.3): userland binary calls `write(1, ...)`.
- **M5 — VFS** (end of Month 4): `cat` a file from a RamFS.

---

## Progress Log

Most recent entries at the top. One entry per work session.

### YYYY-MM-DD
- **Done:** (what got built/learned this session)
- **Status:** (which milestones/checkboxes flipped)
- **Notes / blockers / surprises:** (anything useful for future-you)
- **Next:** (where to pick up next time)

<!-- Template — copy and fill in.
### 2026-06-13
- **Done:** Initialized repo, wrote LEARNING.md and CLAUDE.md.
- **Status:** All roadmap items still `[ ]`. M0 not started.
- **Notes:** Starting from phil-opp post 1 (freestanding binary).
- **Next:** Set up Cargo project, nightly toolchain, freestanding `_start`.
-->
