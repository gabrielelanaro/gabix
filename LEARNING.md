# Learning Plan: mini-os

Goal: learn Linux internals by re-implementing a Linux-like bare-metal OS in Rust.

Approach: follow the "Writing an OS in Rust" tutorial (phil-opp.com) as the spine, and at each milestone read the corresponding Linux source so the abstractions we build connect to real Linux.

## Roadmap

Status legend: `[ ]` not started · `[~]` in progress · `[x]` done

### Month 1 — Skeleton & boot
- [x] 1.1 Freestanding Rust binary, `no_std`, custom panic handler
- [x] 1.2 `bootimage` / `bootloader` crate setup, boot in QEMU
- [x] 1.3 VGA text-mode buffer, print "Hello, World"
- [x] 1.4 GDT (Global Descriptor Table)
- [x] 1.5 IDT + interrupt handlers
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

### What a kernel actually is

A kernel is just a program. Same as Chrome, `ls`, any binary — code compiled to bytes. The difference from a normal program isn't *what* it is, it's *what's beneath it*:

- Normal program: code → OS → hardware. The OS provides files, threads, memory.
- Kernel: code → hardware. Nothing in between. The kernel IS the OS.

When a kernel wants to print to the screen, it can't call `printf` — there's no `printf`. It writes bytes directly to memory addresses wired to hardware (e.g. `0xB8000` for VGA).

### What "compiling a kernel" means

Same as compiling any program. The compiler doesn't know it's a kernel. The difference is which defaults you turn off, because every default assumes an OS exists:

- `#![no_std]` — don't link the standard library (it assumes an OS).
- `#![no_main]` — don't generate the `main` wrapper (no C runtime to call it).
- Custom target JSON — produce a binary shaped for a *bootloader* to load, not an OS loader.

Output is still a normal binary file. Different contents and shape; same structure.

### Binary file formats: ELF / PE / Mach-O

Different OSes use different binary formats. Same concept (structured file with code + data + metadata), different layouts. Historical accident — different teams in the 80s/90s.

| OS | Format | Magic signature |
|---|---|---|
| Linux, Unix | ELF | `\x7FELF` |
| Windows | PE (Portable Executable) | `MZ` |
| macOS | Mach-O | `0xFEEDFACF` (64-bit) |

Magic signatures are how loaders identify formats — the first bytes fingerprint the file. Can't run a Linux binary on Windows directly because the Windows loader can't parse ELF.

For mini-os we use ELF: the `bootloader` crate expects it, and it's the most-documented format for `no_std` Rust.

### Symbol tables

A name → address dictionary inside a binary file. The CPU doesn't need names — only bytes. The table exists for:

- The **linker** — to resolve cross-file references (your code calls `printf`, linker looks it up).
- **Debuggers** — to translate addresses to names (breakpoints, backtraces).
- **You** — to read crash reports.

You can strip the symbol table (`strip binary`) and the program runs identically. Proves it's metadata for tools, not for the CPU.

### `_start` and entry points

`_start` is the convention for "the address where execution begins in this binary." Set by the C runtime decades ago; the GNU linker defaults to using it as the entry point.

How it gets used:

1. You write `_start` in code.
2. Compiler puts it in the symbol table (`#[no_mangle]` stops Rust renaming it).
3. Linker looks up `_start`, gets its address.
4. Linker writes that address into the ELF header's entry-point field.
5. Bootloader reads the ELF header, jumps to the address.

In a normal program the C runtime provides `_start` (does setup, then calls `main`). In a kernel there's no C runtime — you write `_start` yourself. It can't return (return to what?), so its type is `extern "C" fn _start() -> !`.

### The bootloader's job (one sentence)

> Read the kernel file off disk, find where its first instruction is (using whatever format the file is in), load the relevant bytes into RAM, jump there.

The format is just the vocabulary the bootloader uses to find the entry point and figure out which bytes go where in RAM. After the jump, the bootloader is done and the kernel runs.

### The Intel SDM (Software Developer's Manual)

The source of truth for x86-64. Multi-volume PDF (~5000 pages) published by Intel documenting every register, instruction, data structure, and rule the CPU enforces. Use it as a reference, not a tutorial — search by section number when you need an exact bit layout or semantics.

**Landing page:** https://www.intel.com/sdm

**Volumes that matter for OS work:**

| Vol | Title | When to open |
|---|---|---|
| 1 | Basic Architecture | background reading — registers, modes, data types |
| 2 | Instruction Set Reference | looking up a specific instruction's exact semantics |
| 3 | System Programming Guide | **the OS bible** — GDT, IDT, paging, exceptions, multitasking |
| 4 | Model-Specific Registers | per-CPU MSR reference (model-dependent) |
| 5 | Architecture Extensions | rarely (SGX, TSX, etc.) |

For mini-os, **Volume 3** is what we'll open ~90% of the time. Key sections we'll hit:

- §3.4 — Segment descriptors (GDT entry layout) — **M1 GDT slice**
- §6 — Exception and interrupt handling (IDT, fault types) — **M1 IDT slice**
- §4 — Paging (page tables, PTE bits) — **M2**
- §7 — Task management (TSS) — partial, **M1**

**Alternative for 64-bit stuff:** AMD64 Architecture Programmer's Manual, Volume 2 (System Programming). Often clearer because AMD designed the 64-bit extension. https://developer.amd.com/resources/developer-guides-manuals/

**How to use it:** Ctrl-F the PDF by section number. The bit-field diagrams are the gold standard — they're literally what hardware implements. When a tutorial disagrees with the SDM, the SDM wins.

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

### What is the stack, really?

The stack is just regular RAM. The CPU doesn't own any "stack memory" — it has a register (RSP) that holds the address of the current stack top. Push/pop, call/ret, iretq all read/write at [RSP] like any other memory access.

- The "stack" is purely a software convention enforced by the calling convention. The hardware just provides RSP + push/pop instructions.
- The stack grows **down** (toward lower addresses). `push` decreases RSP; `pop` increases it.
- Size is not fixed — it's "as big as the free memory below RSP." Overflow walks into other data → corruption.

There's one CPU-controlled exception: the **IST (Interrupt Stack Table)**, part of the TSS, holds up to 7 alternate stack addresses for critical interrupts where the current stack might be corrupt (e.g., double fault). We don't use IST yet.

### How the bootloader picks the stack address

There's a contract: by the time the bootloader jumps to `_start`, RSP must point at a valid stack. The bootloader honors this by:

1. Querying the firmware (BIOS INT 0x15 / UEFI `GetMemoryMap`) for a memory map.
2. Finding a free region of the right size (~64 KB typically).
3. Mapping it into the kernel's virtual address space (high-half: `0x10000...` virtual → physical RAM).
4. Setting RSP to the **top** of that region (stack grows down).
5. Jumping to `_start`.

The address is **not hardcoded** — it adapts to whatever the firmware reports. This is why QEMU on any host works without changes.

Our `bootloader = "0.9"` crate does all this. We can read the chosen address via `BootInfo::stack_top` if we change `_start` to accept it. For now we just inherit RSP and don't query it.

### CPU modes — why the weird names

The names come from different eras by different teams, so there's no consistent scheme.

- **Real mode** = "real addresses" — what you compute is the physical RAM address, no translation, no protection. Named retroactively when protected mode appeared.
- **Protected mode** = "memory protection exists now" — CPU enforces rules (rings, segment permissions, paging). Intel 286/386 era.
- **Long mode** = "longer registers/addresses" — 64-bit. Named by AMD in 2000 (AMD64). Intel called the same thing "IA-32e mode."

On boot, the bootloader walks the CPU through these in order: real → protected → long. After that, the OS lives in long mode permanently.

x86 has rings 0 (kernel) and 3 (user). ARM has **Exception Levels** instead: EL0 (user), EL1 (kernel), EL2 (hypervisor), EL3 (TrustZone secure monitor). Same idea, different jargon.

### Segments and the GDT (long mode)

**Mental model:** in 64-bit long mode, a "segment" is a *permission envelope over the entire address space*, not a pointer to a specific memory location. All segments cover 0 to 2⁶⁴-1. They differ only in permissions (executable? writable?) and privilege tier (ring 0 / ring 3).

So asking "where is the segment?" doesn't have a specific answer — the segment IS everywhere. The segment is a *rule*, not a *thing*.

**Why it's weird (the history):**

- **16-bit real mode (1978):** segments were actual memory ranges. CPU computed `physical = segment_base × 16 + offset`. You switched segments to access different memory.
- **32-bit protected mode (1985):** same idea, with paging layered on.
- **64-bit long mode (AMD64, 2000):** AMD said "this is stupid, all segments are flat now." The GDT entry *format* still has base/limit fields (for backward compat), but the CPU **ignores them** in long mode. Only the permission bits matter.

**What's actually in a long-mode GDT entry (8 bytes):**

| Field | Long-mode behavior |
|---|---|
| Base (32 bits, split across the entry) | Ignored — treated as 0 |
| Limit (20 bits, split across the entry) | Ignored — treated as "full address space" |
| Type | Code (exec/read) or data (read/write) |
| S | 1 = code/data, 0 = system (TSS, etc.) |
| DPL | Privilege tier (0 = kernel, 3 = user) |
| P | Present — does this entry exist |
| L | 1 = 64-bit code segment |

**So what distinguishes our two kernel segments?**

Both cover all of memory. They differ only in:
- Kernel code segment: executable + read, ring 0
- Kernel data segment: read + write, ring 0

**Where the kernel code physically lives:** wherever the bootloader loaded it (typically `0x100000`, 1MB physical). The segment doesn't describe that location — it describes the *permissions* for whatever location the CPU happens to be executing.

**Why install our own GDT (when the bootloader already has one):**

1. The bootloader's GDT is in memory we don't own. Eventually we'll use that RAM for heap or page tables and the CPU will read garbage.
2. We need to add entries later (TSS in the IDT slice, user segments in the syscall slice). Can't add to the bootloader's GDT.
3. Explicit contract with the CPU: these segments exist, nothing else.

Right now, swapping bootloader's GDT for ours is invisible — same rules, same behavior. We do it for forward-looking reasons.

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

### Rust targets and `no_std`

**Target** = (architecture + OS + ABI) description Rust compiles *for*. Written as a triple like `arch-vendor-os-abi`:

| Target triple | Means |
|---|---|
| `aarch64-apple-darwin` | Apple Silicon Mac on macOS (our host) |
| `x86_64-unknown-linux-gnu` | Linux on x86-64 |
| `x86_64-pc-windows-msvc` | Windows |
| `wasm32-unknown-unknown` | WebAssembly |
| `x86_64-unknown-none` | x86-64 with **no OS** — bare metal |

Rust ships precompiled libraries for ~80 built-in targets. Our `x86_64-mini-os.json` is a **custom target spec** because we want kernel-specific settings (`panic=abort`, `disable-redzone`). Treat the JSON as boilerplate; field names come from compiler internals.

**Rust's standard library is layered:**

```
┌──────────────────────────────────────────┐
│ std         ← File I/O, threads, network,│  needs an OS
│             mutexes, time, println!      │
├──────────────────────────────────────────┤
│ alloc       ← Box, Vec, String, Rc, Arc  │  needs heap allocator, no OS
├──────────────────────────────────────────┤
│ core        ← Option, Result, Iterator,  │  no OS, no heap
│             slices, math, atomics        │
├──────────────────────────────────────────┤
│ compiler_builtins  ← memcpy, memset,     │  compiler emits calls to these
│             integer division, float ops  │
└──────────────────────────────────────────┘
```

`#![no_std]` opts out of **`std` only**. You *keep* `core` and `compiler_builtins` — both OS-independent. So in `no_std` code you can still use `Option`, `Result`, `Iterator`, `&[T]`, `&str`, integer math, atomics. You lose: `Vec`/`String`/`Box` (need `alloc` + heap), `File`/threads/`println!` (need OS).

**Why we need `build-std` in `.cargo/config.toml`:**

Rust ships precompiled `core` for built-in targets only. There's no precompiled `core` for our custom `x86_64-mini-os` target. `build-std = ["core", "compiler_builtins"]` tells the compiler to compile these from source for our target on first build (then cached).

**Why `compiler-builtins-mem`:**

When Rust emits code that copies a slice, it calls `memcpy`. Normally the host OS's libc provides `memcpy`. We have no libc. The feature makes `compiler_builtins` provide its own Rust-native `memcpy`/`memset`/`memmove`/`memcmp`.

**Where this comes back later:** Month 2, when we add a heap allocator, we'll add `extern crate alloc;` to bring in `Box`/`Vec`/`String`. Still `no_std`, but more than just `core`.

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

### 2026-06-14 (slice 7) — Alignment fix (naked asm wrapper)
- **Done:** Fixed the `movaps` alignment fault that prevented interrupt handlers from doing real work. Replaced `extern "x86-interrupt" fn` with a `#[unsafe(naked)] unsafe extern "C" fn` wrapper that uses `naked_asm!` to: `sub rsp, 8` (realign to 16), `lea rdi, [rsp+8]` (pass `&InterruptStackFrame`), `call {inner}`, `add rsp, 8` (undo), `iretq`. The inner function is plain `extern "C" fn` — can use any registers, format strings, println!, etc.
- **Status:** Verified: `int3` now fires, handler prints `EXCEPTION: BREAKPOINT` + the InterruptStackFrame dump, then execution resumes with "It did not crash!". The `extern "x86-interrupt"` ABI is no longer used anywhere — removed `#![feature(abi_x86_interrupt)]` from main.rs.
- **Notes / blockers / surprises:**
  - Recent Rust changes: `#[naked]` is now `#[unsafe(naked)]`; naked functions must use `naked_asm!` not `asm!`.
  - The wrapper doesn't preserve caller-saved registers (rax, rcx, rdx, rsi, rdi, r8–r11). Fine for `int3` (interrupted code doesn't care), but needs fixing before production handlers like page fault.
  - Casting the naked wrapper to `HandlerFunc` requires `core::mem::transmute` — the IDT API only accepts the typed `HandlerFunc`. The CPU doesn't care about ABI distinctions, only the address.
- **Next:** 1.6 hardware interrupts (timer). The handler pattern is now reusable for any IDT entry.

### 2026-06-14 (slice 6) — IDT + breakpoint handler (with detour)
- **Done:** Created `src/interrupts.rs` with a lazy_static IDT and an `init()` that loads it via `lidt`. Registered a breakpoint (#BP) handler for vector 3. Triggered it from `_start` via `x86_64::instructions::interrupts::int3()` to verify the IDT works end-to-end.
- **Status:** Checkbox 1.5 flipped to `[x]`. M1 partially in progress (1.6 hardware interrupts and 1.7 panic stack trace still pending).
- **Notes / blockers / surprises:**
  - Needed `#![feature(abi_x86_interrupt)]` at the top of main.rs — the `extern "x86-interrupt"` ABI is unstable.
  - **Big detour:** first version with `println!` inside the handler triple-faulted. Diagnosed via QEMU logging (`qemu-system-x86_64 ... -d int -D qemu.log`). The `extern "x86-interrupt"` ABI on current Rust nightly generates a prologue that saves XMM registers via `movaps`, which requires 16-byte stack alignment. The CPU delivers interrupt handlers an 8-byte-aligned stack (after pushing the 40-byte interrupt frame), and the compiler's prologue doesn't realign. So `movaps` faults → #GP → no GP handler → #DF → no DF handler → triple fault → reboot loop.
  - **Workaround for now:** handler is empty (just `iretq`). No prologue, no XMM save, no fault. The IDT mechanism works (verified: `int3` fires, handler runs, `iretq` returns, "It did not crash!" prints).
  - **Future fix:** write a naked-asm wrapper that realigns the stack before calling the actual Rust handler. Standard pattern in production Rust kernels. Will be needed before handlers can print (page fault in M2, etc.). Wrote up the full diagnosis in `docs/alignment-crash.html`.
- **Next:** Either fix the alignment so handlers can print, or proceed to 1.6 (timer interrupts) with empty handlers for now.

### 2026-06-14 (slice 5) — GDT
- **Done:** Installed our own GDT. Added `x86_64 = "0.15"` crate. Created `src/gdt.rs` with a `lazy_static!` GDT containing null + kernel code + kernel data segments, and an `init()` that calls `load()` then reloads CS/DS/SS/ES/FS/GS. Called `gdt::init()` from `_start` before the `println!`s.
- **Status:** Checkbox 1.4 flipped to `[x]`. Still at M0 (interrupts need 1.5–1.7).
- **Notes / blockers / surprises:**
  - `x86_64` 0.15 renamed `add_entry` → `append` (older phil-opp blog uses `add_entry`).
  - `CS::set_reg` is special vs other segment registers — can't `mov` to CS, needs an internal far-return trick. The crate handles it; caller API is the same.
  - Swapping GDT is invisible: bootloader's GDT had the same "kernel code + data, ring 0" rules we install. Visible behavior identical (same `Hello, World!`). Only "proof" of correctness is no triple fault.
  - Key conceptual breakthrough: in long mode, segments are *permission envelopes over the entire address space*, not specific memory ranges. Base/limit fields in the GDT entry exist but are ignored.
- **Next:** 1.5 IDT — Interrupt Descriptor Table. Tells the CPU "when exception/interrupt N fires, call this handler." First slice where we actually handle CPU exceptions.

### 2026-06-13 (slice 4) — Writer abstraction
- **Done:** Moved VGA buffer code into `src/vga_buffer.rs` as a typed module. Added `Color`/`ColorCode`/`ScreenChar`/`Buffer`/`Writer` types, scrolling, newline handling, `fmt::Write` impl, global `WRITER` guarded by a spin `Mutex`, and `print!`/`println!` macros. `_start` now calls `println!` instead of hand-rolling pointer writes.
- **Status:** No new roadmap items flipped (this is a refactor of 1.3, not a new milestone). Still at M0.
- **Notes / blockers / surprises:**
  - Dropped the `volatile` crate — its current API requires `T: Deref` for `.write()`/`.read()` on `Volatile<T>`, which doesn't make sense for our `ScreenChar`. Switched to `core::ptr::write_volatile`/`read_volatile` directly.
  - `lazy_static!` is needed because recent Rust const evaluation rejects `&mut *(0xb8000 as *mut Buffer)` — the raw integer "has no provenance." lazy_static defers construction to first use, sidestepping the check.
  - `spin::Mutex` works in `no_std` because it uses a spinlock instead of OS-level mutex primitives.
- **Next:** M1 = interrupts (1.4 GDT, 1.5 IDT, 1.6 hardware interrupts, 1.7 panic with stack trace).

### 2026-06-13 (slice 3) — MILESTONE M0
- **Done:** Milestone M0 reached. Wrote to the VGA text buffer at `0xB8000` (25 rows × 80 cols × 2 bytes per cell). First iteration used naive `*vga.add(N) = ...` pointer writes — visible "Hello, World!" but fragile (compiler could elide "dead" writes since we never read them). Fixed by switching to `core::ptr::write_volatile`, which tells LLVM the writes have side effects and must not be optimized away.
- **Status:** Checkbox 1.3 flipped to `[x]`. **M0 reached** (boots in QEMU + prints to screen). Tagged `m0-hello-world`.
- **Notes / blockers / surprises:**
  - VGA buffer at `0xB8000` is just memory-mapped I/O. Each cell = 2 bytes (ASCII + color attribute). No syscalls, no `printf` — just write bytes to a magic address.
  - Color byte: high 4 bits = bg, low 4 bits = fg. `0x0a` = light green on black.
  - `volatile` is for telling the compiler "this memory access has side effects" — needed for all MMIO. Will become important again when we touch APIC, NIC, disk controllers.
- **Next:** Build a proper `Writer` struct (volatile crate, ScreenChar/Buffer types, `fmt::Write` impl so we can use `write!` macro) before adding interrupts in M1.

### 2026-06-13 (slice 2)
- **Done:** Booting in QEMU. Installed QEMU 11.0.1 (`brew install qemu`) and `bootimage` 0.10.4 (`cargo install bootimage`). Added `bootloader = "0.9"` to Cargo.toml and configured `bootimage runner` in `.cargo/config.toml`. `cargo run` builds a 51KB bootable disk image (`bootimage-mini-os.bin`) and launches QEMU. Black screen = success — SeaBIOS → bootloader crate → our `_start` → `loop {}`.
- **Status:** Checkbox 1.2 flipped to `[x]`. M0 not yet (need VGA + "Hello, World" in 1.3).
- **Notes / blockers / surprises:** None. `bootloader 0.9.34` + `bootimage 0.10.4` work fine with current nightly.
- **Next:** Slice 3 = VGA text-mode buffer + "Hello, World". This is milestone M0.

### 2026-06-13 (slice 1)
- **Done:** Scaffolded Cargo project. `cargo build` produces a freestanding x86-64 ELF binary with `_start` symbol at `0x201120`. Hit three snags from recent nightly changes (1.98, 2026-06-13): JSON target specs need `-Zjson-target-spec`, numeric fields must be unquoted, `soft-float` target feature is rejected (deprecated) and removing it requires also removing `-mmx,-sse` because modern `core` needs SSE2 (it's part of the x86_64 ABI).
- **Status:** Checkbox 1.1 flipped to `[x]`. M0 still in progress.
- **Notes / blockers / surprises:**
  - phil-opp's target spec JSON is out of date for current nightly. Modern working spec: unquoted numbers, no `features` line at all.
  - Apple Silicon host — QEMU not yet installed, only matters for slice 2.
- **Next:** Slice 2 = add `bootimage` + `bootloader` crates, install QEMU, get it booting (still no printing).

### 2026-06-13
- **Done:** Initialized repo, wrote LEARNING.md and CLAUDE.md. Background reading session covered: what a kernel is (just a program with no OS beneath); how compiling a kernel differs from a normal program (`no_std`, `no_main`, custom target); binary file formats (ELF / PE / Mach-O); symbol tables; `_start` and entry points; the bootloader's one-sentence job; boot flow; CPU modes (real/protected/long, rings vs ELs); paging tables with the file-folder analogy; ARM vs x86-64. All distilled into the Concepts reference section.
- **Status:** All roadmap items still `[ ]`. M0 not started.
- **Notes:** Mental model that ties it all together: a kernel is just a program with no OS beneath it; every config tweak is about telling the compiler "don't assume an OS." Concepts reference is for lookup, not memorization.
- **Next:** Scaffold Cargo project, install nightly, write freestanding `_start`.
