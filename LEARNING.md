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

## Concepts reference

Distilled notes from background reading. Come back here when terms feel fuzzy.

### Bootloader vocabulary

| Term | Meaning |
|---|---|
| **Real mode** | CPU's 1978 16-bit mode. No protection, real addresses. Where the CPU starts. |
| **Protected mode** | 32-bit. Memory protection exists (GDT, rings, paging). |
| **Long mode** | 64-bit. Requires paging. Where modern kernels live. |
| **BIOS** | Old firmware (1980s–2010s). Loads 512-byte boot sector. Legacy. |
| **UEFI** | Modern firmware. Loads an `.efi` executable directly. |
| **MBR / boot sector** | First 512 bytes of a disk. BIOS loads these. |
| **GDT** | Global Descriptor Table — CPU structure describing memory segments. |
| **Paging** | Virtual memory. Map virtual → physical addresses. Required for long mode. |
| **A20 line** | Historical hack to address more than 1MB. Legacy cruft. |
| **Multiboot / Multiboot2** | Standard contract: "if your kernel has this header, GRUB can boot it." |
| **Stage 1 / Stage 2** | Bootloaders bootstrap themselves: tiny stage 1 loads bigger stage 2. |

### How a PC boots

```
[Power on]
   ↓ CPU wakes up in real mode (always, for backward compat)
[BIOS / UEFI firmware runs] — initializes hardware, finds bootable disk
   ↓
[Loads first 512 bytes (MBBR) of disk into RAM at 0x7C00, jumps there]
   ↓
[Bootloader stage 1] — too small to do much, loads stage 2
   ↓
[Bootloader stage 2] — switches real → protected → long mode, sets up GDT + paging, reads kernel off disk
   ↓
[Kernel entry point (_start)] — runs in long mode with paging on
```

In a **micro VM** (e.g. Firecracker), the VMM skips firmware AND bootloader: it directly loads the kernel into guest RAM, sets the initial CPU state to "long mode, paging on, RIP at kernel entry" via the KVM API. The kernel wakes up thinking a bootloader just handed off — but none of the legacy dance happened. Real parallel: Linux's `kexec()` syscall.

### CPU modes — why the weird names

The names come from different eras by different teams, so there's no consistent scheme.

- **Real mode** = "real addresses" — what you compute is the physical RAM address, no translation, no protection. Named retroactively when protected mode appeared.
- **Protected mode** = "memory protection exists now" — CPU enforces rules (rings, segment permissions, paging). Intel 286/386 era.
- **Long mode** = "longer registers/addresses" — 64-bit. Named by AMD in 2000 (AMD64). Intel called the same thing "IA-32e mode."

On boot, the bootloader walks the CPU through these in order: real → protected → long. After that, the OS lives in long mode permanently.

x86 has rings 0 (kernel) and 3 (user). ARM has **Exception Levels** instead: EL0 (user), EL1 (kernel), EL2 (hypervisor), EL3 (TrustZone secure monitor). Same idea, different jargon.

### Paging tables

Virtual memory = the address your code uses is not the address in physical RAM. The CPU translates on every memory access. The translation rules live in **page tables**.

**Mental model:** page tables are like nested folders. To find a piece of data, you start at the top folder, open the next, open the next, ..., eventually reach a leaf that points to actual data.

**Acronyms spelled out:**

| Acronym | Stands for | What it is |
|---|---|---|
| **PML4** | Page Map Level 4 | Top-level folder. One per process. |
| **PDPT** | Page Directory Pointer Table | Second level. |
| **PD** | Page Directory | Third level. |
| **PT** | Page Table | Bottom level. Entries point to physical pages. |
| **PTE** | Page Table Entry | One row inside any table. |
| **MMU** | Memory Management Unit | Hardware in the CPU that does the lookup. |
| **TLB** | Translation Lookaside Buffer | Cache of recent lookups so MMU doesn't re-walk the tree every time. |
| **CR3** | Control Register 3 | CPU register holding the physical address of the PML4 (top folder). Switching processes = updating CR3. |

**The walk:** for a 48-bit virtual address, the CPU splits it into 4×9-bit indexes + 12-bit offset. Each 9-bit index selects one of 512 entries in the current level's table. After 4 lookups you reach the physical page; add the offset to get the byte.

**Why 512?** Each table fits in one 4KB page, each entry is 8 bytes, so 4096/8 = 512.

**What it enables:** process isolation (each process has its own tables), lazy allocation, copy-on-write fork, mmap, swap, kernel/user address space split.

**Where to read Linux doing this:** `mm/page_alloc.c` (buddy allocator for physical pages), `mm/memory.c` (page fault handler), `include/linux/mm.h`.

### ARM vs x86-64 (quick comparison)

| | x86-64 | ARM64 (AArch64) |
|---|---|---|
| Privilege levels | Rings 0–3 | Exception Levels EL0–EL3 |
| Boot modes | Real → protected → long (legacy dance) | Wakes up directly in AArch64, no legacy |
| Boot standard | Universal (BIOS/UEFI + MBR/EFI) | Fragmented — each SoC vendor does their own |
| Top-level page table reg | CR3 (one) | TTBR0 (user) + TTBR1 (kernel) — two separate tables |
| Page size | 4KB (or 2MB / 1GB huge) | 4KB, 16KB, or 64KB (configurable) |
| Instruction style | CISC, variable length 1–15 bytes | RISC, fixed 4-byte length |
| GP registers | 16 (RAX–R15) | 31 (X0–X30) |
| I/O ports | Yes (`in`/`out`) | No — memory-mapped only |
| Interrupt controller | LAPIC / IOAPIC | GIC (Generic Interrupt Controller) |
| Memory ordering | Strong (TSO) | Weak — needs explicit barriers |

**For mini-os:** we target x86-64 (phil-opp's tutorial ecosystem is x86-64). Even on Apple Silicon, QEMU emulates x86-64 fine for learning.

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

### 2026-06-13
- **Done:** Initialized repo, wrote LEARNING.md and CLAUDE.md. Background reading session covered boot flow, CPU modes, paging tables, ARM vs x86. Added distilled reference to "Concepts reference" section.
- **Status:** All roadmap items still `[ ]`. M0 not started.
- **Notes:** Starting from phil-opp post 1 (freestanding binary). Don't need to memorize concepts reference — it's there for lookup when terms feel fuzzy.
- **Next:** Scaffold Cargo project, install nightly, write freestanding `_start`.
