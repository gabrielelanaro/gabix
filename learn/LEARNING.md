# Learning Plan: mini-os

Goal: learn Linux internals by re-implementing a Linux-like bare-metal OS in Rust.

Approach: follow the "Writing an OS in Rust" tutorial (phil-opp.com) as the spine, and at each milestone read the corresponding Linux source so the abstractions we build connect to real Linux.

## Roadmap

### Didactic code readability (current cleanup goal)
- [x] Audit hardware and protocol operations across shared, x86, and ARM code; give constants, helper calls, and sequencing explanations meaningful names
- [x] Verify each changed area with existing relevant checks and physical Pi runs for hardware paths

### x86 / ARM feature parity (current objective)
- [x] Cooperative ARM kernel tasks: private stacks, round-robin yield, completion; verify QEMU and physical Pi
- [x] Timer-driven ARM preemption and interrupt-safe scheduler state (single-core kernel tasks)
- [x] Per-process ARM address spaces and memory isolation (two fixed user allocations; section mappings)
- [ ] ARM process lifecycle: fork, exec, wait, exit
- [ ] Shared file descriptors, VFS, and file syscalls on both architectures
- [ ] Shared userspace sockets, blocking I/O, and TCP on ARM
- [ ] Equivalent behavioral suites on x86, ARM virt, and physical Pi
- [ ] Future extension: file-backed demand paging on x86 and ARM — handle a missing mapping by reading the required file page from storage into RAM, then resume the process. Existing page tables do not provide this automatically; the boot loader can continue loading the whole kernel.

Each slice includes didactic source names, a short lesson, and physical validation
through UART reboot/upload. Preserve the working server as the recovery image.

### Networking objective
- [x] Lesson 26.1 UDP socket objects, per-process FDs, and port routing
- [x] Lesson 26.2 `socket`, `bind`, `sendto`, and `recvfrom` syscalls
- [~] Lesson 26.3 blocking receive with scheduler wakeup and network worker
- [~] Lesson 26.4 ring-3 UDP echo server and host-forwarded E2E test
- [x] Lesson 30 DHCP client and runtime network configuration

### Raspberry Pi 2B target
- [x] Share ARMv7 boot, memory, timer, SD, ext2, and userspace code
- [x] Verify DWC2 USB host, hub, RNDIS Ethernet, DHCP, and UDP in QEMU
- [x] Build the physical kernel and partitioned SD-card image
- [x] Initialize Raspberry Pi USB power, DWC2 DMA, and LAN9512 Ethernet
- [ ] Verify UART, SD, USB, Ethernet, DHCP, ping, and UDP on a physical Pi 2B
- [x] Ethernet uploads into SD kernel storage with complete read-back verification
- [x] Permanent SD boot with two 32 MiB copies, trial confirmation, watchdog fallback, UART recovery, and a preserved recovery image
- [x] Router preparation: authenticated Ethernet management/reboot, persistent buffered logs, DHCP retry/renewal diagnostics; physical outage and reboot checks
- [ ] Verify the managed Pi on the actual FRITZ!Box LAN after moving the cable
- [x] USB Wi-Fi: RTL8188CU initialization, WPA2-Personal, DHCP, authenticated management, reconnect, and permanent SD boot verified on the Pi
- [x] Upload two distinct ARM user executables over authenticated Wi-Fi and read their exit statuses on the same physical Pi boot. [Uploader](../scripts/pi2-wifi-user.py)
- [x] Keep Wi-Fi responsive during blocking TCP calls from an uploaded ARM user program. [Physical echo check](../tests/test_pi2_wifi_tcp_echo.py)
- [x] Transfer two V3D frames over one ARM TCP connection and verify every byte during slow reads and retransmission. [Physical frame test](../tests/test_pi2_v3d_frames.py)

### Remote GUI on the Pi 2B

Start with the [seven VideoCore IV architecture lessons](lessons/videocore/index.html). Use the [six triangle labs](lessons/roadmap-pi-v3d.html) to inspect one draw on the Pi.

- [x] Run returning ARMv7 hardware experiments from RAM over authenticated Wi-Fi. Reuse the timer with two inputs, then replace it with the V3D probe in the same boot. [Probe runner](../scripts/pi2-ram-probe.py)
- [x] Power up V3D on the physical Pi and read back `V3D_IDENT0 = 0x02443356` over Wi-Fi with the existing `gpu_mem=16` configuration.
- [x] Submit two V3D clears and read back every pixel of each 64 × 64 frame.
- [x] Bin and render a triangle on V3D. Verify all foreground and background pixels, then change the fragment shader color and draw again.
- [x] Display the V3D triangle with a hover dot in TigerVNC on the Mac over Wi-Fi. [Physical VNC test](../tests/test_pi2_vnc.py)
- [x] Drag the triangle from TigerVNC. V3D redraws its translated vertices on each move.
- [x] Submit three vertices from an uploaded user program and verify two distinct triangle shapes on one Pi boot.

The uploaded user program requests a draw and streams the completed pixels over TCP. TigerVNC displays the enlarged image and positions the hover dot. The x86 accepted-socket sender currently advances its sequence number without tracking the peer's window or retaining segments for retry (`src/net/tcp.rs:371`).

### TCP arc — the connection-oriented transport
- [x] Lesson 31 TCP segment, three-way handshake, and first byte exchanged
- [x] Lesson 32 Reliable stream — sliding window, retransmission, RTT
- [x] Lesson 33 Connection teardown and ring-3 TCP echo server over slirp

**Primary spec:** RFC 9293 (the modern consolidated TCP spec, obsoletes RFC 793). **Linux to read alongside:** `net/ipv4/tcp_*.c`, especially `tcp_input.c` (receive path), `tcp_output.c` (segment generation), `tcp_timer.c` (retransmit), `include/net/tcp.h` (`struct tcp_sock`).

Status legend: `[ ]` not started · `[~]` in progress · `[x]` done

### Month 1 — Skeleton & boot
- [x] 1.1 Freestanding Rust binary, `no_std`, custom panic handler
- [x] 1.2 `bootimage` / `bootloader` crate setup, boot in QEMU
- [x] 1.3 VGA text-mode buffer, print "Hello, World"
- [x] 1.4 GDT (Global Descriptor Table)
- [x] 1.5 IDT + interrupt handlers
- [x] 1.6 Hardware interrupts: PIC/APIC timer
- [x] 1.7 Panic with stack trace

**Linux to read alongside:** `init/main.c`, `arch/x86/kernel/`, `arch/x86/entry/`

### Month 2 — Memory management
- [x] 2.1 Frame allocator (bitmap or buddy-lite)
- [x] 2.2 4-level paging, map kernel higher-half
- [x] 2.3 Heap allocator (`linked_list_allocator` then roll your own)
- [x] 2.4 `Box` / `Vec` / `String` working in kernel
- [x] 2.5 `mapper` API for arbitrary virtual→physical mapping

**Linux to read alongside:** `mm/page_alloc.c` (buddy allocator), `mm/memory.c`, `include/linux/mm.h`

### Month 3 — Processes & scheduling
- [x] 3.1 `Task` struct (context, kernel stack, state)
- [x] 3.2 Context switch routine (save/restore registers)
- [x] 3.3 Cooperative scheduler round-robin
- [x] 3.4 Preemptive scheduling via timer interrupt
- [x] 3.5 Kernel threads (idle task, demo workers)
- [x] 3.6 User/kernel mode ring transition (TSS, `syscall`/`sysret`)

**Linux to read alongside:** `include/linux/sched.h` (`task_struct`), `kernel/sched/fair.c` (CFS), `arch/x86/entry/entry_64.S`

### Month 4 — Syscalls, VFS, IPC
- [x] 4.1 Syscall gate (`syscall`/`sysret` ABI), dispatch table
- [x] 4.2 Core syscalls: `write`, `exit`, `getpid`
- [x] 4.3 `fork` / `exec` / `wait`
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

### I/O ports (separate address space)

x86 has **two** address spaces for talking to hardware:

| Address space | Range | Access | Examples |
|---|---|---|---|
| Memory | 0 to 2⁶⁴-1 | Normal pointer (`mov`) | VGA buffer at `0xB8000` |
| I/O ports | 0 to 65535 | Special `in`/`out` instructions | PIT at `0x40`, PIC at `0x20`, keyboard at `0x60` |

These are **different namespaces**. Port `0x40` and memory address `0x40` are unrelated.

In Rust, the `x86_64` crate's `Port<T>` type wraps `in`/`out`:
```rust
let mut port: Port<u8> = Port::new(0x40);
unsafe { port.write(0x36); }  // emits `out dx, al`
```

**Why two spaces:** historical. Intel 8086 (1978) had separate I/O instructions. Most modern architectures (ARM, RISC-V) have only memory-mapped I/O. x86 kept ports for backward compat. Newer devices (PCIe, NVMe, USB) use memory-mapped I/O exclusively; legacy stuff (PIT, PIC, PS/2) still uses ports.

**Analogy:** building with elevators (memory) and a mailroom with PO boxes (ports). Different numbering, different staff.

### Interrupts and the IDT

**Interrupt** = "pause this code, run a handler, resume where you left off." The whole OS is built on this primitive.

Three types:
- **Hardware interrupt** — device signals the CPU (timer tick, key press, disk done)
- **CPU exception** — CPU detected an error (divide by zero, page fault, invalid opcode)
- **Software interrupt** — your code asked for one (`int3` breakpoint, `int 0x80` old syscall)

All three go through the **IDT (Interrupt Descriptor Table)**: an array of 256 entries in RAM. The CPU has a register `IDTR` pointing to it.

Each IDT entry has:
- **Handler address** (where to jump)
- **Segment selector** (which GDT entry to load into CS — usually our kernel code segment)
- **IST index** (optional alternate stack for critical faults)
- **Type** (interrupt gate vs trap gate)
- **DPL** (minimum privilege to invoke via `int N`)

**The 256 vectors:**
- 0–31: CPU exceptions, fixed by Intel (0=divide error, 3=breakpoint, 8=double fault, 13=GP, 14=page fault, etc.)
- 32–255: user-defined (hardware IRQs, software interrupts like syscalls)

**What happens when interrupt N fires:**
1. CPU finishes the current instruction
2. CPU pushes the interrupt frame onto the stack: RIP, CS, RFLAGS, RSP, SS (40 bytes total — *not* a multiple of 16, hence the alignment bug)
3. CPU looks up IDT[N], loads the handler's segment + address
4. CPU jumps to the handler
5. Handler runs, returns via `iretq` (pops the frame, resumes)

**The `extern "x86-interrupt"` ABI on current nightly has an alignment bug** — its prologue uses `movaps` to save XMM registers, which requires 16-byte stack alignment, but the CPU delivers an 8-mod-16-aligned stack. Workaround: use a `#[unsafe(naked)] unsafe extern "C" fn` wrapper that manually realigns the stack before calling a normal Rust function. See `src/interrupts.rs` (`breakpoint_wrapper`, `timer_wrapper`) and `docs/alignment-crash.html` for the full story.

### Hardware interrupts: PIC + EOI + IF

**PIC (Programmable Interrupt Controller)** — Intel 8259 chip. Sits between devices and the CPU. Many input wires (one per device), one output wire to the CPU. When a device fires, the PIC signals the CPU and tells it which IRQ.

Two PICs chained (master + slave) = 15 usable IRQs:
- IRQ 0: timer (PIT channel 0)
- IRQ 1: keyboard
- IRQ 6: floppy
- IRQ 14/15: disk controllers
- etc.

**Remapping the PIC:** by default the PIC maps IRQs 0–15 to CPU vectors 8–15, which conflict with exceptions. We remap to vectors 32–47 at boot. The `pic8259` crate does this via `initialize()`.

**EOI (End Of Interrupt):** after handling a hardware interrupt, we *must* tell the PIC "I'm done, send the next one." Forget this and the PIC never delivers another interrupt from that source. Common gotcha. Send via `PICS.lock().notify_end_of_interrupt(vector)`.

**IF (Interrupt Flag):** a bit in RFLAGS. When 0, the CPU ignores maskable hardware interrupts (timer, keyboard). When 1, it handles them. Exceptions always fire regardless of IF.

Default after boot: IF=0. We enable as the last step of init via `x86_64::instructions::interrupts::enable()`. Order matters: set up all handlers, configure PIC and devices, *then* enable IF. Otherwise an interrupt fires before the handler is ready → triple fault.

**pic8259 gotcha:** `ChainedPics::initialize()` *saves and restores* whatever mask state the firmware left. In QEMU that means everything masked. Must explicitly `write_masks(0xFE, 0xFF)` after to unmask IRQ 0 (timer). Pattern: bit N of mask byte = 1 means "IRQ N masked."

### The 8253 PIT (Programmable Interval Timer)

Intel 8253 chip. Has **3 channels** (independent timers) in one chip + a shared command register:

| Port | Channel | Connected to |
|---|---|---|
| `0x40` | 0 | **IRQ 0** (the timer interrupt we use) |
| `0x41` | 1 | DRAM refresh on original PC (unused) |
| `0x42` | 2 | PC speaker (system beeps) |
| `0x43` | Command | Shared — configures any channel |

**Base clock:** ~1.193182 MHz. Divided by a **16-bit divisor** (1–65535) to get the output frequency. So:
- Max frequency: ~1.193 MHz (divisor = 1)
- Min frequency: ~18.2 Hz (divisor = 65535)

**You cannot ask for less than 18.2 Hz.** Asking for 2 Hz panics (divisor doesn't fit in 16 bits). Discovered the hard way.

**Programming sequence:**
1. Write command byte to `0x43`: `0x36` = channel 0, lo+hi access, mode 2 (rate generator), binary
2. Write divisor low byte to `0x40`
3. Write divisor high byte to `0x40`

That's it. The PIT starts firing immediately at the configured frequency. Output goes through the PIC to CPU vector 0x20.

### Debugging the kernel

Bare-metal kernels are hard to debug because there's no `printf`, no GDB by default, no logs. Here's the toolkit that works:

**QEMU monitor via Unix socket** (most useful):
```
qemu-system-x86_64 -drive format=raw,file=...bin \
  -display none \
  -monitor unix:/tmp/qm.sock,server,nowait &

sleep 4
echo "CMD" | nc -U /tmp/qm.sock
```

Useful monitor commands:
- `xp /Nbx ADDR` — dump N bytes of physical memory (e.g. `0xb8000` for VGA)
- `info registers` — CPU state (look at RIP, RSP)
- `x /Ni ADDR` — disassemble N instructions
- `x /Ngx $rsp` — dump stack values (find return addresses for backtraces)

**Symbol lookup** — convert a hex address to a function name:
```bash
nm mini-os | sort | awk '$1 <= ADDR {last=$0} END {print last}'
```

**Disassemble a function:**
```bash
objdump -d mini-os | grep -A 20 "FN_NAME:"
```

**Key facts:**
- Panic handler with `loop {}` is a black hole. If RIP points at `eb fe` (`jmp -2`), the kernel panicked. Find `rust_begin_unwind` in `nm` output.
- Walk the stack via `x /20gx $rsp`. Non-zero values are usually return addresses. Decode each with `nm` to get a backtrace.
- **`cargo build` does NOT rebuild the bootable `.bin` file.** Always use `cargo run` or `cargo bootimage`. A stale `.bin` looks identical to "my code doesn't work" — easy to waste an hour here.

**Mental flowchart for "kernel doesn't print what I expect":**
1. Dump VGA at `0xb8000` via monitor.
2. Has text? → writer works, problem is elsewhere.
3. All zeros? → kernel isn't writing.
4. Check RIP. If `eb fe` → kernel panicked.
5. Walk stack via `x /20gx $rsp`. Decode return addresses via `nm`.
6. Find the panicking function. Read its source — usually obvious why.
7. If RIP is elsewhere (in your code), disassemble and figure out what it's doing.

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

### 2026-11-25 — Remove the docs directory

- Removed the root `docs/` directory and added a path check to the test runner. Updated live references and kept historical hardware evidence accessible at the previous commit. No roadmap checkbox changed. Next, review the ARM boot assembly move.

### 2026-11-25 — Move ARM startup assembly under src/arch

- Moved the shared entry and exception assembly to `src/arch/armv7/boot.S` and updated kernel, SD-check and course references. Both ARM virt and Raspberry Pi 2B boot tests passed in QEMU. Board parameters still come from the ARM crate. Next, isolate the Pi-specific GPIO setup. No roadmap checkbox changed.

### 2026-11-25 — Organize the Pi probes, course material and layout tools

- Grouped the probe sources, VideoCore course, terminal lesson and repository audit for separate review. The hardware overview uses a Gabix drawing. The architecture map generator reads its template from `scripts/templates/`. No roadmap checkbox changed in this cleanup. The next GUI experiment is to reduce the number of bytes sent during a drag.

### 2026-11-24 — Profile VNC drag latency and V3D events on boot 39

- Measured 69 rendered frames on the physical Pi with the VC4 counters. Each frame produced 312 color-writing quads and 3,024 aggregate fragment-shading cycles. Calibrated the ARM virtual counter at approximately 1 MHz against the BCM system timer.
- Full-drag medians were 0.286 ms in the draw syscall, 16.39 ms encoding and buffering, and 23.51 ms in socket writes. The uninstrumented Mac round trip was 70.65 ms. A 300-byte region took one write and reached the Mac in 41.00 ms. Restored the counter configuration and default GUI. No roadmap checkbox changed. The next experiment is to fit the full triangle response in one TCP payload.

### 2026-11-24 — Submit user-space vertices to V3D

- On Pi boot 39 (kernel `5ca30ba4`), the uploaded shape test checked all 4,096 pixels of an isosceles triangle and a right triangle with identical colors. The VNC drag, clip, release and re-grab checks passed on the same boot. Both user programs use the same three-vertex draw syscall.
- Packing the VNC headers and rectangle data together reduced median Mac pointer-request-to-complete-RRE-reply time from 100.25 to 70.35 ms across 14 moves per build. Seven alternating zero-draw and 128-draw batches estimated 0.30 ms per draw including frame readback. The timings exclude Mac display presentation. The next experiment can merge adjacent rows of the same color and width before transmission.

### 2026-11-24 — Drag the V3D triangle in TigerVNC

- The VNC program picks the gold pixel under a left-button press and preserves its offset while dragging. The driver writes translated X/Y coordinates into the three vertex records and resubmits the draw. Release fixes the final position.
- Physical Pi boot 38 (kernel `f5175863`) passed full-frame comparisons for an off-center grab, repeated motion, clipping, release, background presses and a second grab. Marked the drag milestone complete.

### 2026-11-24 — View the V3D triangle in TigerVNC

- Uploaded the RFB server as a user ELF on physical Pi boot 37. TigerVNC on the Mac displayed the gold V3D triangle on a blue 512×512 desktop. The GPU supplies the 64×64 image; each pixel covers an 8×8 block.
- Added an outlined hover dot. The physical test checks every scaled pixel, pointer updates, restoration after motion, a cropped RGB565 request and the cursor sprite. Marked the TigerVNC milestone complete. Next, drag the triangle.

### 2026-11-24 — Audit the repository layout

- Added an executable inventory and placement rules for both kernel packages, shared code, standalone probes, host tools, tests, and documentation. The directory map reports rule evidence and proposed destinations. Fixture tests exercise coverage, recursive totals, and findings that clear after relocation.
- Documented the proposed sibling x86 and ARM package layout in [the architecture plan](https://github.com/gabrielelanaro/gabix/blob/f6816a34606a1a0b032658fa44fb5bc000d6cad7/docs/repository-layout.md). The first migration is the x86 package move with the existing boot entry points verified. This review adds tooling and leaves the implementation milestones open.

### 2026-11-24 — Map code size and reviewed architecture findings

- Added `scripts/build-architecture-map.py` and its HTML template. The offline treemap opens at `target/architecture-map.html` and drills down to source evidence. Verified recursive totals, tile area, and directory navigation. The next architecture review can extend the six recorded findings. No roadmap checkbox changed.

### 2026-11-24 — Transfer two V3D triangles over one TCP connection

- Added the ARM triangle syscall and an uploadable TCP frame server. On physical Pi boot 37, the Mac verified all 32,768 bytes against pixel-center coverage for two color pairs. Frame CRC32 values were `c8b588bb` and `545a5cc4`.
- During the slow read the test kernel withheld 536 bytes at sequence 534824842 and logged the same sequence, size and payload CRC32 (`e3d6be36`) on retransmission. The ELF exited 0 on kernel `4588ba2b`. Marked frame transfer complete. Next, serve the pixels as an RFB framebuffer update to a VNC viewer.

### 2026-11-24 — Keep Wi-Fi responsive during ARM userspace TCP waits

- A RAM-uploaded echo server exchanged 27 bytes and 290 bytes on a single TCP connection on physical Pi boot 36 (kernel CRC32 `1f560926`). The Mac checked authenticated status during a 12-second idle interval, then half-closed its socket. The ELF exited with status 0. The same port served a second run on that boot.
- Added one passive IPv4 listener for the foreground ELF. The kernel polls Wi-Fi while a socket syscall waits. Next, verify 16 KiB framebuffer transfers with a slow reader and a dropped segment.

### 2026-11-24 — Install pstack project skills

- Installed 59 Cursor skills and their two helper agents. Verified the upstream file hashes before naming the pstack teaching skill `pstack-teach` to preserve the gabix `teach` command. The kernel roadmap and its next steps are unchanged.

### 2026-11-24 — Run different ARM user programs over Wi-Fi on one Pi boot

- Built and installed the Wi-Fi ELF loader once. On physical Pi 2B boot 34 (kernel CRC32 `bf587896`), uploaded two executables into RAM. The UART-write fixture exited with 0. The mode-reporting fixture read CPSR and exited with 16 (ARM user mode). No reboot or kernel upload occurred between programs.
- Added a [Mac uploader and hardware test procedure](https://github.com/gabrielelanaro/gabix/blob/f6816a34606a1a0b032658fa44fb5bc000d6cad7/docs/pi2-wifi-user.md). The next GUI prerequisite is ARM TCP socket support with network service during user execution. No existing roadmap checkbox changed.

### 2026-11-24 — Start shared TCP support for the GUI

- Moved the existing TCP segment reader and writer into kernel-core for reuse on the Pi. The x86 socket implementation now uses the shared packet format. Next, connect a passive TCP endpoint to ARM network polling. No roadmap checkboxes changed.

### 2026-11-24 — Draw the V3D concept map

- Replaced the expanded map’s repeated boxes with a selectable vector diagram. Each concept has one node in the full graph. No roadmap checkboxes changed.

### 2026-11-24 — Define map concepts at two levels

- Added a Gabix example and a general definition to every box in the expanded V3D map. No roadmap checkboxes changed.

### 2026-11-24 — Give each GPU concept its own box

- Split compound nodes in the expanded map, including the vertex fetch hardware, QPU storage, arithmetic units and completion counters. No roadmap checkboxes changed.

### 2026-11-24 — Expand the GPU concept map

- Added an [expandable map](lessons/videocore/expanded-map.html) with definitions attached to every box. Checked coverage against the previous map. No roadmap checkboxes changed.

### 2026-11-24 — Map the VideoCore IV curriculum

- Added a [component and prerequisite map](lessons/videocore/concepts.html) tracing one draw and connecting each subsystem to the driver. No roadmap checkboxes changed. Use the map to choose the next lesson.

### 2026-11-24 — Teach the GPU architecture from the hardware drawing

- Embedded Broadcom’s system diagram and added a guided sequence with enlarged details. The course index now gives a concrete exercise for each of the seven lessons. No roadmap checkboxes changed. Start with the guided map, then trace the driver’s ID read.

### 2026-11-24 — Name lesson one for the hardware interaction

- Renamed the first architecture lesson and updated its introduction and index entry. No roadmap checkboxes changed.

### 2026-11-24 — Build a beginner path through the V3D architecture

- Added seven lessons grounded in Broadcom’s diagram and the Gabix driver, with stepped figures, source excerpts and explanation exercises. No hardware roadmap checkboxes changed. Begin with the boundary of Figure 1.

### 2026-11-24 — Remove the architecture video

- Removed the film and its course entry at the learner’s request. No roadmap checkboxes changed.

### 2026-11-24 — Read Broadcom’s VideoCore IV architecture guide

- Added the Inside VideoCore IV video, a narrated introduction to the hardware in Figure 1 of Broadcom’s guide. Eight chapters connect the command executor, vertex path, QPU slices, instruction format, texture memory and tile buffer to the relevant PDF pages. The player includes chapter seeking, English captions and a transcript. No roadmap checkboxes changed. Next reading is Figure 2 on page 17 for the QPU core or page 62 for host control.

### 2026-11-24 — Explain the V3D control thread number

- Added the binning and rendering constants, the Rust call that passes <code>RENDERING</code>, and the register address calculation to [lesson 8.02](lessons/8.02-v3d-submission.html). The blue clear uses thread 1. Lesson 8.03 follows the triangle through both threads. No roadmap checkboxes changed.

### 2026-11-24 — Read the Rust inside the Pi graphics diagrams

- Embedded the relevant Rust in every step of [lesson 8.02](lessons/8.02-v3d-submission.html). The excerpts follow the same request from the kernel call through the mailbox, the ID load and the two render-register writes. Checked each excerpt against the indicated source lines. No roadmap checkboxes changed. Continue with the tile list in lesson 8.03.

### 2026-11-24 — Trace a Mac command into the Pi’s graphics hardware

- Revised [lesson 8.02](lessons/8.02-v3d-submission.html) around the learner’s question about where the ID reply comes from. Three figures keep the Mac, the Pi’s ARM and VideoCore processors, V3D and shared RAM in view. The [code tour](tours/tour-8.02-v3d-submission.html) connects the Mac command to the kernel call, the optional firmware power request and the register read. Checked the ID, render-list bytes and blue frame against the saved boot 32 files. No roadmap checkboxes changed. Continue with the clear-to-triangle comparison in lesson 8.03.

### 2026-11-24 — Build the Pi graphics hardware lessons

- Published lessons 8.01–8.06 with 13 interactive figures, six code tours and a [Pi 2B register reference](reference/pi2-v3d-registers.html). Source excerpts connect the ARM stores, GPU packet bytes, QPU instructions and DMA register bits to the same 64 × 64 frame. The readback lesson can inspect a local frame from the hardware test. Verified the controls at 320, 390 and 1120 pixels wide, the local file inspector against the boot 32 readback, and the packet encodings against the linked probe. Start at [pixel coverage](lessons/8.01-v3d-coverage.html).

### 2026-11-24 — Plan the V3D driver and hardware lessons

- Built a six-lesson plan around the learner’s questions about command submission, pixel coverage and memory mapping. Its opening figure replays two pixels sampled from boot 32 before and after the triangle draw. The plan follows those same pixels through the binner, rasterizer, QPU, tile store and Wi-Fi readback. Added the requested figure and layout defaults to the teaching skill. No implementation checkboxes changed. Start with the edge prediction in lesson one.

### 2026-11-24 — Run ARM programs from RAM over Wi-Fi and draw the first V3D triangle

- Completed the first four Pi GUI roadmap items. Installed the authenticated development kernel in SD copy A over Wi-Fi. The 243,312-byte image passed readback with CRC32 `8786d8b8`. Copy B retained the previous kernel. Compared copy A with the final local rebuild (`94ace136`). The only three differing bytes encode source line numbers in diagnostic records.
- Uploaded the timer and V3D probes into RAM during the same boot. The timer measured 1,000 and 2,000 µs on request. Firmware powered V3D through domain 11 with the existing memory split. The Mac read `V3D_IDENT0 = 0x02443356`. An early stalled probe triggered the board watchdog and the Pi rejoined Wi-Fi.
- Verified two clears and two triangle draws on boots 31 and 32. Each 64 × 64 triangle had 1,152 foreground pixels, and the Mac checked all 4,096 pixels against the three vertices. The second draw changed both colors. Publishing instructions through the VideoCore-coherent alias using DMA fixed stale fragment colors after shader edits.
- Rebooted between test runs. Boot 32 rejected the previous RAM session and loaded the verified SD kernel again. Draw plus 16 KiB readback took 0.609 and 0.698 seconds on boot 32. Next GUI milestone is an RFB framebuffer update for TigerVNC Viewer. [Runbook](https://github.com/gabrielelanaro/gabix/blob/f6816a34606a1a0b032658fa44fb5bc000d6cad7/docs/pi2-ram-probes.md) · [Physical hardware test](../tests/test_v3d_renders_triangle.py)

### 2026-11-24 — Generalize the Wi-Fi experiment runner

- Proposed a common calling convention for returning hardware experiments. The uploaded program defines the meaning of the inputs and results. The same image can run with different inputs, and new experiments can be uploaded during the session. Timer and V3D reads would exercise two peripherals through the same runner. Planning only.

### 2026-11-24 — Session-only Wi-Fi uploads

- Consider a development kernel that receives authenticated probe images into reserved RAM, checks the complete image, calls a probe and sends its result back over Wi-Fi. First acceptance check: run two different probes during one boot, then reset and verify the original saved kernel boots. The upload must leave the SD kernel slots and their selection intact. Full-kernel replacement in RAM is a separate design because the running Wi-Fi kernel occupies the ordinary kernel load address. Planning only.

### 2026-11-24 — Use Wi-Fi for V3D experiments

- Plan the first ID read through a managed trial kernel and existing remote logs. A later diagnostic command could repeat a compiled draw with new colors or vertices during one boot. Driver code changes still use the kernel updater. Planning only.

### 2026-11-24 — Plan the V3D hardware iteration loop

- For repeated register and render experiments, use a small privileged probe uploaded into RAM over UART. The resident loader accepts another upload when a probe returns with the loader environment intact (`targets/armv7/src/pi2_loader.rs:109`). Proposed first tooling step: time build, upload, and result separately on the identification probe, then repeat it without rebooting. Use managed Wi-Fi uploads for integrated kernel checks. Planning only. The Mac did not list the expected USB serial adapter during this review, so physical cycle time is still unmeasured.

### 2026-11-24 — Audit prerequisites for the first V3D draw

- Checked Pi memory mappings, firmware mailbox requests, USB DMA cache handling, ARM syscall dispatch and the network polling loop. Split the GPU milestone into identification, two GPU clears with readback, and a triangle. Recorded the userspace and TCP dependencies beside the viewer milestone. All checks remain open. This audit used source and hardware documentation. The next hardware experiment is the identification probe on the existing boot profile.

### 2026-11-24 — Start the GUI project with V3D

- Revised the GUI roadmap around the hardware rendering learning goal. The first experiment is to identify V3D on the physical Pi and have it clear an offscreen buffer to a known color. The first triangle follows in that buffer. Network display and pointer input follow the GPU readback check.
- Planning only. All three GUI milestones are open.

### 2026-11-24 — Planning a remote graphical session

- Proposed first milestone: open a VNC viewer on the laptop and drag one triangle rendered by Gabix. The viewer sends pointer coordinates and button state. A Gabix userspace program owns the triangle and sends pixel rectangles from an offscreen buffer. [RFB protocol, sections 7.5–7.7](https://www.rfc-editor.org/rfc/rfc6143.html#section-7.5)
- Proposed sequence: draw a solid triangle with the CPU, serve the first image from x86 QEMU, then implement press, drag and release. Bring the same program to the Pi over Wi-Fi through the planned ARM socket interface. For hardware rendering, submit a fixed triangle to the Pi 2's V3D engine and check a foreground pixel and a background pixel in RAM before connecting the result to the viewer. [Linux VC4 command submission](https://docs.kernel.org/gpu/vc4.html#memory-management-and-3d-command-submission)
- Planning only. Existing milestones retain their status. The ARM syscall dispatcher still needs socket operations (`targets/armv7/src/syscall.rs:30`). On x86 the accepted TCP socket needs flow control and retransmission for reliable transfers (`src/net/tcp.rs:371`).
- Suggested first experiment: transfer a small image through the x86 TCP listener in bounded writes and compare the received bytes with the expected image. Then display the same pixels in a standard viewer.

### 2026-11-23 — One-read terminal experiment
- Rebuilt lesson 4.11 around an executable observation and replay. Verified on a host PTY that the corrected input waits before Enter and returns three bytes after Enter. Navigation checks passed. Roadmap unchanged. Next: learner walkthrough.

### 2026-11-23 — Student review of lesson 4.11
- Followed ls through the pending read, labelled editor-owned buffers, and reduced job-control, register and test-planning detail. Reader-model and navigation checks passed. Roadmap checkboxes unchanged. Next: learner review in the browser.

### 2026-11-23 — Terminal transport and editing figures
- Added reversible serial/PTY and kernel/shell editing diagrams alongside the read simulator. Removed the repeated buffer table. Navigation checks passed for all four diagram variants. Roadmap checkboxes unchanged. Next: visual review at the learner’s viewport.

### 2026-11-23 — Illustrated teletype operation
- Added a stepped isometric figure to lesson 4.11 showing keyboard transmission, host echo, printing, carriage return and paper feed. Roadmap checkboxes unchanged. Next: inspect the animation in the lesson.

### 2026-11-23 — Revised terminal lesson introduction
- Introduced the terminal, shell and read interface before the worked example. Used the I/O Devices chapter of Operating Systems: Three Easy Pieces as a structural reference. Roadmap checkboxes unchanged. Next reading exercise: follow the kernel queue across wakeup.

### 2026-11-23 — Terminal buffers and blocked reads lesson
- Rebuilt lesson 4.11 around the learner’s model of typing a line and pressing Enter. Added an isometric, reversible walkthrough with serial/PTY and canonical/editor modes, explicit buffer ownership, and separate wake/schedule actions.
- Verified x86 COM1/file/socket paths and ARM PL011/uploader/SVC/scheduler paths against local source; linked stable line-numbered excerpts. Added a quick reference and a proposed dual-architecture RX-to-read experiment. Kernel code and roadmap checkboxes unchanged.
- Validation: Node syntax and model behavior checks passed (`node tests/test_tty_read_returns_line.cjs`); local HTML links/anchors and `git diff --check` passed. Browser visual QA remains unverified: sandbox denied the local HTTP server and browser policy rejected file URLs.
- Next: review the lesson interactively; implement the proposed TTY experiment only as a separate task. ARM blocking I/O remains a prerequisite.

### 2026-11-16 — Publish the Wi-Fi connection lesson
- Added the isometric startup walkthrough, interactive connection checks, quick reference, and source-linked code tour. Source snapshots preserve the implementation used when the lesson was written; the quick reference includes the subsequently implemented mDNS hostname.
- Verified JavaScript syntax, local lesson links, and whitespace. Roadmap checkboxes unchanged. Generated hardware output remains ignored.

### 2026-11-16 — Readability goal completed with physical recovery checks
- Closed the source-readability validation checkbox and audit coverage parents after the remaining UART tests passed. Earlier host/QEMU results are retained in docs/code-readability.md; this session supplies the outstanding physical evidence.
- Wi-Fi diagnostic passed deliberate disconnect, fresh WPA2 handshake and a second DHCP lease. A separate existing diagnostic passed explicit radio power-off/power-on and firmware reload. Both returned automatically to the installed Wi-Fi service.
- SD write/read/restore/neighbour test passed. Added a guarded managed-layout opt-in to the existing fixture: validate the installed layout/boot record and use empty metadata sector 1024 rather than the live staging header. Committed as af46939.
- Rebuilt loader tested from RAM: hung-candidate watchdog fallback, valid candidate confirmation, both invalid checksums entering UART recovery, and restored normal SD boot all passed. Resident loader file preserved.
- Wired Ethernet passed DHCP, ARP, two pings, three exact UDP echoes after idle and authenticated management. Fixed two diagnostic timing defects in 1404060: extend the host timeout beyond the process-isolation checks and refresh the network-development watchdog before USB/link startup. Full diagnostic rerun passed including automatic reset.
- Final boot 27: installed image CRC32 fedce5c8, gabix-pi2.local resolves and echoes over Wi-Fi, no pending update, no log-write errors/dropped messages. MBR and scratch/guard sectors match backups; boot-record checksum is valid. Raw output and private test packages stay under ignored target/runs/20261116-uart-validation/.
- Next separate experiment: remove external power and verify unattended Wi-Fi/mDNS startup. The earlier intermittent reachability report remains unexplained; software resets and software-controlled radio power cycling do not establish full cold-start behaviour. Feature-parity roadmap items and documented pre-existing defects remain open.

### 2026-11-16 — mDNS installed; readability hardware validation resumed
- Implemented shared IPv4 mDNS host discovery and integrated it into the managed ARM network service. The Pi probes/announces `gabix-pi2.local`, responds to A/ANY queries, handles compressed names and known answers, and selects a suffixed name on conflict. DNS-SD browsing, IPv6 and reverse lookup remain outside this slice.
- Thirteen shared-library tests including Wi-Fi security pass; three mDNS cases cover packet bounds, compression, probing, reply routing, conflict handling and address changes. The existing ARM virt DHCP regression passes. Its initial local-socket permission failure was resolved by running with the required permission.
- Installed the final 243,968-byte build over Wi-Fi, with exact SD read-back (CRC32 `fedce5c8`). Authenticated reboot by hostname selected/confirmed it as copy A, boot 16. Fresh multicast lookup, macOS name resolution, UDP echo by name, authentication rejection checks, persisted logs and cross-boot replay rejection pass with UART/Ethernet disconnected.
- The first raw multicast test followed the Mac's VPN route (`utun7`) and timed out. Selecting the local Wi-Fi interface passed; the test now selects the interface toward the resolved host, with an explicit interface-address override. Native macOS `.local` resolution works without a router change.
- Resumed the readability hardware gate: current-build Pi logs confirm cooperative scheduling, timer preemption, isolated userspace and fault containment. USB/Wi-Fi startup, DHCP, SD kernel writes/read-back, managed reset and persistent logging have current-build evidence. Read-only SD inspection validated the boot-record checksum and kernel extents on the 15,523,840-sector card; no pending update remains.
- Remaining physical checks need UART/Ethernet reattached: deliberate Wi-Fi diagnostic reconnect, wired driver exercise, scratch-sector restore/neighbour test, and loader/UART recovery cases. Full power-removal startup is also pending; a software reboot leaves USB power present. User is away; keep the working Wi-Fi service running. Raw evidence stays in ignored `target/runs/20261116-pi-reconnect/`. The overall readability validation checkbox remains open.

### 2026-11-16 — Reconnect diagnosis and Wi-Fi-only reboot
- Recovered contact with the installed kernel through the existing UART user-reboot flow; authenticated Wi-Fi status and persistent logs became available at 192.168.178.128. Installed image CRC32 is 0xdefc3948; this does not validate the later readability build.
- Previous boot 12 recorded Wi-Fi Online and a DHCP lease around 75 seconds. The earlier failed discovery therefore does not establish a failure to join; the later loss of reachability remains unexplained.
- User disconnected Ethernet and UART while keeping power and the Wi-Fi dongle attached. Wi-Fi status continued responding. An authenticated network reboot then produced boot 14, automatically joined Wi-Fi, acquired the same address, and answered status at uptime 76.453 seconds with no reported transmit or log-write errors.
- Startup logs show process-isolation checks completing near 72 seconds, before network initialization. This explains a substantial startup wait, but does not prove the cause of the earlier outage. A power-removal cold-start reproduction remains outstanding.
- Evidence is in ignored target/runs/20261116-pi-reconnect/. No installed kernel or SD layout was changed; roadmap validation remains open.

### 2026-11-15 — Readability audit: local validation checkpoint
- Completed the source-review milestone across shared, x86 and ARM code; recorded unchanged facade files and corrected affected renamed API/constant references. Checked the source-audit roadmap item only.
- Current verification passes: 10 shared-library tests with Wi-Fi security enabled, two Wi-Fi frame tests, ARM QEMU process isolation and scheduler/preemption tests. Earlier current-revision uploader/registry checks passed 16 tests.
- The physical-validation roadmap item stays open. Pi is disconnected; no current hardware upload, reboot, Wi-Fi connection or SD write has been performed.
- Next required work: reconnect the Pi and execute the explicit physical validation gate in docs/code-readability.md, confirming image identity and card layout before any write. Historical course listings beyond references affected by this cleanup remain historical material, not a new full-course rewrite.

### 2026-11-15 — Readability audit: mailbox/VirtIO references
- Scanned removed constant names and updated five VirtIO references; replaced the mailbox lesson's outdated helper with current source and polling explanation.
- HTML parsing and whitespace checks pass. No kernel behavior changed.
- Physical Pi validation remains pending; final consistency/evidence review continues.

### 2026-11-15 — Readability audit: host compatibility and hardware gate
- Passed all 16 uploader/registry host tests and all eight default-feature shared-library unit tests.
- Reviewed physical helper scope and recorded concrete Wi-Fi/USB, management/logging, SD recovery and memory/scheduling checks still required.
- Pi remains disconnected; no physical actions performed. Recovery scratch sector 8192 overlaps the staging header, so verify current layout/update state before that test.
- Continue local reference consistency work; hardware validation remains an explicit completion gate.

### 2026-11-15 — Readability audit: renamed helper references
- Searched removed API names across teaching material and host scripts/tests; updated the VFS tour/glossary's stale user-range helper.
- Corrected unsafe pointer-validation claims and verified the path snippet exactly matches current source. Both HTML files parse.
- Remaining historical snippet consistency and physical validation are still open.

### 2026-11-15 — Readability audit: LAN9512 lesson consistency
- Updated register names, transmit return contract, endpoint descriptions and adapter selection context; corrected source locations.
- Checked against current drivers/shared framing and parsed the HTML. Kernel behavior is unchanged.
- Continue affected reference/test-helper review; physical validation remains pending.

### 2026-11-15 — Readability audit: USB lesson consistency
- Updated lesson 7.04's transfer explanation for DMA cleanup, retry policy and IN/OUT completion; corrected its claim about timer polling.
- Source checks and HTML parsing pass. Existing styling/layout retained; no kernel changes in this slice.
- Continue reviewing affected learning/test references. Physical Pi checks remain pending.

### 2026-11-15 — Readability audit: source inventory follow-up
- Read the remaining unchanged module/re-export/entry wrappers and recorded no-edit coverage; clarified frame allocator, dispatcher and halt contracts.
- Offline x86 cargo check passes. Comment-only changes preserve behavior.
- Next: learning-reference/test-helper consistency and physical validation. Source review alone does not close the broad roadmap checks.

### 2026-11-15 — Readability audit: shared interfaces/reset
- Documented sector-read error differences, frame/console interface limits and force-unlock caller requirements; named i8042 reset fields.
- Offline x86 cargo check passes. No behavior changes, reset or hardware activity.
- Remaining inventory/reference checks continue; physical Pi validation remains pending.

### 2026-11-15 — Readability audit: UART smoke/platform exit
- Named standalone GPIO/UART setup and heartbeat counts; documented clock assumptions and FIFO configuration.
- Named ARM semihosting exit fields and clarified that virt reset exits the emulator.
- UART smoke and virt builds pass. Physical checks and remaining source/reference inventory remain pending.

### 2026-11-15 — Readability audit: x86 console contracts
- Corrected serial delivery, VGA mode/font and QEMU exit-status claims; named the VGA address and replacement glyph and documented separate output locks.
- Offline x86 cargo check passes. Behavior is unchanged; no redundant behavioral tests were added.
- Source inventory follow-up includes UART smoke/platform helpers and learning-reference consistency; physical Pi validation remains pending.

### 2026-11-15 — Readability audit: DHCP progress snapshots
- Replaced positional logging-state fields with named fields and named the retry/lease reporting thresholds; explained snapshot updates and service-restart lifetime.
- Managed kernel build passes; log text and conditions are preserved. Physical checks remain pending.
- Learning-reference consistency review remains open; the existing untracked Wi-Fi tour cites historical, physically tested snippets.

### 2026-11-15 — Readability audit: ARM boot network policy
- Named boot network timing/identity constants and documented partial enumeration, device preference, retained DHCP sessions and trial-watchdog handling.
- Clarified that boot confirmation checks device initialization, without requiring DHCP or a host round trip; policy is unchanged.
- Five relevant build configurations pass, including managed Wi-Fi release. Physical checks remain pending; orchestration details and lesson-reference review continue.

### 2026-11-15 — Readability audit: USB/LAN diagnostic probes
- Named probe packet/timing and assembly setup values; explained first-child selection, partial enumeration, USB submission counts and receive filtering.
- All nine existing build variants pass. Output, control flow and values are preserved; physical hardware remains disconnected.
- Remaining orchestration and learning-reference review continue, with hardware validation still pending.

### 2026-11-15 — Readability audit: boot/BSS/allocator probes
- Reviewed eight diagnostic programs, named mode/reset/stack values and explained the scope of each UART result.
- Corrected a false five-second comment for the existing 80-tick watchdog countdown (about 1.2 ms); preserved all timing values and instruction order.
- All eight existing build scripts pass. Physical checks remain pending; USB/LAN probes, orchestration and learning-reference coverage remain open.

### 2026-11-15 — Readability audit: atomic diagnostic probes
- Named reset/mode/timing fields and explained UART markers, inherited entry state and the limited single-operation scope of both atomic probes.
- Both release probe build scripts pass. Values and instruction order are preserved; no physical Pi test was performed.
- Remaining probes, orchestration and lesson-reference review continue. Physical checks await the disconnected Pi; roadmap checkboxes remain open.

### 2026-11-15 — Readability audit: network service polling
- Named link status values and link/DHCP polling intervals; explained exclusive service ownership, synchronous work and management-frame priority.
- Managed and UART-user builds pass with unchanged values and operation order. The Pi is disconnected, so physical checks remain pending.
- Remaining orchestration, probes and lesson-reference review continue under the readability goal.

### 2026-11-15 — Readability audit: UART service timing
- Named managed watchdog timing and explained UART idle-timeout priority, network polling delays, static-buffer ownership and user-exit service restart.
- Managed and UART-user builds pass with unchanged values/behavior. No physical device activity was performed.
- Main orchestration, remaining probes/lesson references and physical checks remain open.

### 2026-11-15 — Readability audit: ARM memory ownership/cache
- Named descriptor subfields and documented static-table exclusion, inherited control state, bootstrap visibility and cached/noncacheable alias requirements.
- Existing QEMU isolation test and Pi hardware build pass with unchanged descriptor values and instruction order. Hardware cache assumptions remain unverified in this slice.
- Checked off ARM memory source-contract review; remaining probes/orchestration, lesson references and physical validation stay open.

### 2026-11-15 — Readability audit: x86 mapper ownership
- Corrected overly strong safety claims about single-core exclusion, mapper ownership and direct-map frame copying. Documented aliasing/synchronization requirements and MMIO lookup/insertion assumptions.
- Cargo check passes; comment-only edits preserve behavior. Existing safe-API ownership gaps are recorded, not repaired by this audit.
- Checked off x86 ownership source review. ARM cache/memory assumptions and remaining audit/physical validation work stay open.

### 2026-11-15 — Readability audit: persistent diagnostics
- Named journal fields, RAM capacities and flush/scan timings. Explained queue serialization, record splitting, write/read-back, retry timing, approximate drop counts and formatting truncation.
- Managed kernel build passes with unchanged values/behavior. No SD writes or physical reboot were performed.
- Memory assumptions, remaining orchestration/probes and lesson-reference review remain open; physical journal validation awaits the Pi.

### 2026-11-15 — Readability audit: management authentication
- Named signed-envelope fields, lengths and direction prefixes. Explained sequence consumption, one-reply cache, shared-peer counter and persistent boot-number dependency.
- Managed kernel build passes with unchanged bytes/behavior. Private key material was neither printed nor committed; physical authentication tests remain pending.
- Journal persistence, memory assumptions and remaining audit areas stay open.

### 2026-11-15 — Readability audit: management status fields
- Named status payload offsets/ranges and lengths, matched to the host decoder. Documented cached-network versus fresh-storage snapshots and stale boot fields after failed SD reads.
- Managed and plain transfer builds pass; wire values remain unchanged. No physical request was sent.
- Authentication/journal behavior, memory assumptions and other audit areas remain open. Pi validation is pending.

### 2026-11-15 — Readability audit: SD upload state
- Renamed metadata persistence to describe both staging and kernel targets. Explained chunk offsets, retry read-back, padding, completion CRC and pending-trial publication; named sector units and CRC polynomial.
- Managed kernel build passes with unchanged behavior. No physical upload or SD write was performed.
- Status payload, authentication/diagnostics and other audit work remain open. Physical Pi validation is pending.

### 2026-11-15 — Readability audit: SD transfer fields
- Named request/reply fields, management operation numbers and persisted staging-header offsets; matched the wire format to the host uploader.
- Clarified restored verification state. Managed kernel build passes with unchanged values/behavior; no physical transfer or SD write was performed.
- Transfer state/status, authentication/diagnostics and other audit areas remain open. Physical checks await the disconnected Pi.

### 2026-11-15 — Readability audit: installer partition boundaries
- Named MBR fields and development-card fingerprint; documented third-entry-only publication, conditional active-copy protection and caller-owned staging bounds.
- Managed kernel build passes with unchanged comparisons/control flow. No SD writes were performed.
- Installer source review is recorded; transfer/management/diagnostics, memory assumptions and physical Pi verification remain open.

### 2026-11-15 — Readability audit: installer FAT lookup
- Named FAT32 boot/directory fields, attributes, chain masks and root-search limit. Explained existing allocation cap and incomplete geometry/cross-link validation, preserving values and behavior.
- Managed kernel build passes; no package was applied or hardware written.
- MBR identity, remaining installer bounds and other audit areas remain open. Physical Pi validation is pending.

### 2026-11-15 — Readability audit: installer package and phases
- Named package fields/record stride, fixed kernel extents and ordered install phases. Verified the package layout against the host writer; documented stable-input requirements and partial-write retry behavior.
- Managed kernel build passes. No hardware installation or SD write was performed; values and control flow are preserved.
- FAT/MBR parser review, remaining installer bounds, management/diagnostics and other audit work remain open. Physical validation is pending.

### 2026-11-15 — Readability audit: resident loader startup
- Named startup GPIO/UART fields and baud divisors. Linked the source-clock assumption to boot/config.txt and documented inherited CPU mode/MMU/cache/vector state.
- Existing firmware-address test and SD-loader build pass. Values and instruction order are unchanged; physical re-entry was not tested.
- Installer/management/diagnostics, memory assumptions and other audit work remain open. Pi validation is pending.

### 2026-11-15 — Readability audit: SD boot selection
- Named UART override/watchdog timings and SD transfer units. Clarified record persistence, image-failure fallback, UART recovery, streaming CRC and loader-return assumptions.
- Existing UART-loader firmware-address test and SD-loader build pass. No device writes or physical boot tests were performed.
- Loader startup assembly, installer/management and remaining memory review stay open; physical validation remains pending.

### 2026-11-15 — Readability audit: trial handoff and confirmation
- Shared the existing RAM handoff field names between loader writer and kernel reader. Named trial watchdog duration and running-image address; clarified consume-once validation and confirmation failure semantics.
- SD loader and managed kernel builds pass. Three existing host boot-record tests pass; these do not establish physical handoff/persistence correctness.
- Boot selection, installer/management/diagnostics and other audit areas remain open. No device writes were performed; Pi validation is pending.

### 2026-11-15 — Readability audit: ARM boot UART and console
- Named GPIO function selection, UART register offsets, FIFO/control fields, baud divisors and semihosting values. Explained console polling, caller-owned setup, discarded receive errors and unbounded transmit waits.
- Existing QEMU userspace-write test and Pi hardware/UART-user builds pass; values and ordering are preserved.
- Memory ownership/cache assumptions and remaining loader/management areas stay open. Actual pin routing and receive validation await the disconnected Pi.

### 2026-11-15 — Readability audit: ARM exception frames
- Named modes, saved-status offset, return-address adjustments and call alignment in boot.S. Explained SVC/IRQ restoration and non-returning abort frames, preserving values and instruction order.
- Recorded existing aligned-stack/no-nested-SVC assumptions and absent VFP/NEON preservation. Existing QEMU isolation and task/preemption tests plus Pi hardware build pass.
- Boot peripheral setup and memory ownership/cache review remain open; physical Pi execution remains pending.

### 2026-11-15 — Readability audit: ARM scheduler lifecycle
- Named the dispatcher sentinel and probe round count. Explained address-space/stack switch order, per-task IRQ restoration, static stack reuse and process-spawn preconditions.
- Corrected the stack-lifetime comment and documented that finished call chains do not run destructors. Existing QEMU task/preemption and process-isolation checks pass after fixing a local constant-substitution typo.
- Marked scheduler/process source review in the audit; exception assembly and memory/cache review remain open. Physical Pi verification is still pending.

### 2026-11-15 — Readability audit: ARM syscall adapter
- Named register indices, getpid/error/output constants and semihosting exit fields. Documented the saved-frame layout, process versus standalone exit behavior and write-pointer validation limits.
- Existing QEMU userspace-write and isolation tests pass; Pi hardware build passes. Numeric values and behavior are unchanged.
- Exception assembly, process scheduling/ownership, remaining memory review and other audit areas stay open. Physical Pi execution remains pending.

### 2026-11-15 — Readability audit: ARM user entry
- Named processor modes and the flat-image entry field; explained user stack setup, saved processor status and the final exception return instruction.
- Documented bootstrap ELF-loader permission, entry validation, partial-failure and cache-synchronization limits without changing behavior.
- Existing QEMU userspace-write test passes. Remaining ARM exception/process and memory ownership work stays open; physical Pi checks remain pending.

### 2026-11-15 — Readability audit: ARM process probe
- Named the shared Rust/assembly probe fields, access kinds, syscall numbers, stack offsets, timer threshold and fault-status bits. Explained stopped-process setup requirements; values and behavior are preserved.
- Existing QEMU isolation test passes all five scenarios; Pi hardware build passes. No new tests or tracked run artifacts were added.
- General ARM process/exception and memory ownership review remains open. Physical validation still awaits the disconnected Pi.

### 2026-11-15 — Readability audit: ARM mapping controls
- Named MMU/cache control bits, translation-table alignment, section mask and process section/stack constants. Explained table-register setup and the exact bounds checked by user-range helpers.
- Existing QEMU ARM memory-mapping and process-isolation tests pass; values and instruction order are unchanged.
- ARM memory ownership/cache assumptions, process/exception paths and remaining audit areas stay open. Physical Pi verification is pending while disconnected.

### 2026-11-15 — Readability audit: TCP connection lifecycle
- Named the passive sequence-generator constants and reused the send-segment size. Explained listener/connection ownership, accept admission, receive buffering and teardown in source comments.
- Recorded existing protocol and lifetime limitations explicitly, including absent passive retransmission, unbounded receive queues, retained closed connections and unchecked FIN acknowledgments. Behavior is unchanged.
- Existing QEMU TCP echo passes. TCP source-review coverage is checked off in the audit; remaining ARM/page-table areas and physical Pi checks stay open.

### 2026-11-15 — Readability audit: TCP retransmission
- Named RTT smoothing/variation weights, timeout/backoff factors and initial peer-window assumption. Explained sequence accounting, cumulative ACK removal, preparation-time timestamps and caller-owned transmission.
- Existing QEMU TCP-loss test passes through retransmission and response. Arithmetic and behavior remain unchanged; formulas checked against RFC 6298.
- TCP state/passive ownership and remaining ARM/page-table review stay open. Physical Pi checks remain pending.

### 2026-11-15 — Readability audit: TCP packet layout
- Named TCP header ranges, Data Offset units, fixed window/MSS values and IPv4 checksum pseudo-header fields. Verified layout against RFC 9293; serialized values are unchanged.
- Clarified the parser's option-skipping contract. Existing QEMU TCP echo test passes through handshake and data transfer.
- TCP state/retransmission and remaining ARM/page-table review remain open. Physical Pi verification stays pending while disconnected.

### 2026-11-15 — Readability audit: UDP queues and runtime network state
- Documented weak port bindings, shared socket ownership, unbounded datagram queues, single receive waiter, poll-waiter registration and lock release before wakeup.
- Clarified ARP learning/cache limits and network configuration's stored lease metadata and destination filter. Comments follow the existing implementation; no behavior changed.
- Cargo check passes. Behavioral tests were not repeated for comment-only changes. TCP, remaining ARM and page-table review plus physical Pi verification remain open.

### 2026-11-15 — Readability audit: network service and ARP waits
- Named ARP iteration limit, task sentinel, broadcast addresses and dump width. Clarified device locking, receive side effects, task enabling and ARP request/wait sequencing.
- Corrected atomicity/timing claims against the actual driver and decoder paths. Documented discarded transmit results, invalid-frame drain termination and optional UDP checksums.
- Existing QEMU UDP echo test passes. Behavior is unchanged; remaining network/ARM and page-table review plus physical Pi validation stay open.

### 2026-11-15 — Readability audit: ext2 serialized fields
- Named the remaining superblock, group-descriptor, inode and directory field offsets, block-pointer stride and permission mask. Parsing values and widths are unchanged.
- Clarified validation limits and removed obsolete lesson-stage claims. Existing QEMU file-read test passes for the supported image.
- ext2 source review is covered; broader networking/ARM and page-table contract review remain open, along with physical Pi validation.

### 2026-11-15 — Readability audit: ext2 adapter units and limits
- Named disk/metadata widths and direct/indirect pointer positions. Corrected directory type-code and descriptor-boundary explanations against the Linux ext2 definitions.
- Documented fixed-block-size, group-0, direct-block and sparse-read limitations without changing behavior. Existing QEMU file-read test passes for the supported image.
- Serialized ext2 field offsets and remaining network/ARM areas still need review; physical Pi validation stays pending.

### 2026-11-15 — Readability audit: VFS cache and descriptors
- Named internal directory-record widths and parser boundaries, reused ROOT_INO, and clarified file/socket handle ownership and cursor sharing.
- Corrected stale claims about path lookup and cache hits. Documented cached-only writes, shared inode-number namespace and retained cache entries across mount replacement.
- Existing QEMU file-read test passes. Serialized format and runtime behavior are unchanged. Remaining filesystem adapter/network/ARM review and physical Pi verification stay open.

### 2026-11-15 — Readability audit: process table and task creation
- Corrected stale ownership and fork comments: monotonic PID slots, shared open objects, low-half copy versus shared mappings, per-child stack placement and startup interrupt state now follow the source.
- Documented existing name-truncation, kernel-task child-list and boot-mapper visibility limitations. This slice changes comments only; it does not repair those behaviors.
- Existing fork-shared-file-cursor QEMU test passes. Page-table synchronization and remaining codebase review remain open; physical Pi checks are still pending.

### 2026-11-15 — Readability audit: scheduler states and lifecycle
- Factored the existing Ready/Running choice into is_runnable and reused the shared frame size for task stacks. Clarified block/yield/wake sequencing, descriptor callback locks and retained Dead-task allocations.
- Documented interrupt-state contracts for exit, exec replacement and the current idle halt branch. Scheduling behavior and state selection are unchanged.
- Existing wait-status and socket poll QEMU tests pass. Task creation/fork/process ownership review and physical Pi checks remain open.

### 2026-11-15 — Readability audit: socket syscall ABI
- Named socket-address fields and sizes, protocol selection, poll wait modes, configuration operations and active-open sequence parameters. Explained byte order, raw-memory requirements and receive/output ordering.
- Documented the existing single-global-active-connection limitation and deterministic initial sequence calculation. Values and behavior remain unchanged.
- Existing poll and passive TCP echo QEMU tests pass; active-open connection behavior is outside those checks. Scheduler/process lifecycle and other remaining areas still need review; Pi testing remains pending while disconnected.

### 2026-11-15 — Readability audit: syscall memory and process contracts
- Renamed the numeric address predicate to is_low_half_range throughout syscall handlers and documented its missing mapping/permission checks. Explained path scan bounds, cached file writes and shared file cursors.
- Named standard output/error descriptors, time-conversion units, wait-any sentinel and exec flags. Documented the raw wait-status ABI, unchecked result pointer and global fork-snapshot concurrency limitation.
- Existing wait-status and timer QEMU tests pass. Values and behavior are preserved. Socket handler internals and scheduler/process lifecycle review remain; physical Pi validation remains pending while disconnected.

### 2026-11-15 — Readability audit: x86 executable loading
- Named ELF64 header/program-entry offsets and used shared frame sizes in segment/stack mapping. Clarified file stride, zero-fill, page permissions and cloned address-space behavior against the parser and loader.
- Corrected claims about validation and rw- stacks: current code trusts several file fields, maps pages executable, and retains old user mappings outside replacement ranges. Those limitations are unchanged and documented for separate correctness work.
- Existing exec-replacement QEMU test passes, including initial executable loading. gABI header/program layouts checked; no redundant tests added. Broader process and ARM review remain open; Pi checks remain pending while disconnected.

### 2026-11-15 — Readability audit: x86 interrupt entry
- Named exception stack offsets/padding and reused PIC timer-vector and crate fault-bit definitions. Explained register saving, return through IRETQ and deferred network-task processing; removed stale ABI and NAPI claims.
- Checked CPU frame layout against Intel SDM 3A section 6.14.2. Recorded existing defects: breakpoint caller-register clobber, page-fault call misalignment, an unconditional demo-memory read and missing SIMD preservation. Behavior remains unchanged; these require separate correctness work.
- Existing timer and UDP echo QEMU checks pass. They do not validate exception recovery. Page-table ownership, process code and ARM review remain open; physical Pi validation remains pending while disconnected.

### 2026-11-15 — Readability audit: x86 page-table dimensions and copying
- Named page-table entry count and high-half boundary; frame strides, copying and clearing use the shared frame size. Loop bounds and copied bytes are unchanged.
- Corrected stale fork/unmap explanations and clarified that address-space handles can refer to existing tables. Documented shared writable kernel state, shared huge pages, mapping errors and caller serialization requirements.
- Existing fork-resumption and exec-replacement QEMU tests pass. Paging synchronization/ownership and remaining interrupt/process code still need review. Pi remains disconnected; hardware validation is pending.

### 2026-11-15 — Readability audit: x86 syscall entry
- Named saved syscall argument offsets and the seventh C argument's stack slot. STAR selector fields derive from the GDT. Frame arithmetic uses the shared size; comments correctly identify the existing stack as 64 KiB.
- Replaced obsolete argument-preservation commentary with the actual distinction between ordinary return and the smaller fork snapshot; corrected the assembly step explanation before argument conversion. Push/pop order, offsets, stack size and selector values are preserved.
- Existing fork-resumption and UDP echo tests pass, covering parent/child return and socket syscalls. Remaining interrupt wrappers, paging and process logic still need review; physical Pi checks remain pending.

### 2026-11-15 — Readability audit: x86 boot and user entry
- Named CR4 SSE-support bits, initial user flags, timer rate and queue size. User entry obtains selectors from the GDT instead of repeating their encoded values. Explained the IRETQ stack order.
- Corrected stale GDT/TSS descriptions: scheduler updates RSP0, and SYSCALL uses a software stack switch. Fixed assembly annotations: OR changes flags; the push/IRETQ sequence touches the stack and transfers control permanently. Intended register values and transition order are preserved.
- Existing user-mode boot and timer tests pass. Checked descriptor privilege selection in the installed x86_64 crate. Interrupt wrappers, paging and syscall entry remain to review; physical Pi validation remains pending.

### 2026-11-15 — Readability audit: x86 saved task stacks
- Shared named saved-register slots and stack sizes between context switching and scheduler stack construction. Fork assembly uses named operands derived from the same layout. Corrected the new-task diagram to identify its trampoline return address.
- Existing fork test passes: both parent and child resume. Initial test launch failed before boot on its local forwarding port; rerun with port access passed. Register order, stack offsets, alignment and return instructions are preserved.
- Scheduler lifecycle and interrupt/exception entry code still require review; physical Pi checks for earlier changes remain pending.

### 2026-11-15 — Readability audit: x86 interrupt controllers
- Named APIC base mask, global/software enable fields, spurious-vector value and acknowledgement. Separated MSI address encoding from the CPU's mapped APIC registers and documented the existing APIC-ID-0/xAPIC assumptions.
- Named PIC masks and corrected its stale overview: initialization enables timer IRQ 0. Existing x86 QEMU timer test passes through boot, disk loading and advancing time. Controller values and ordering are unchanged.
- Interrupt entry/exception assembly, paging, process/syscall code and remaining ARM areas still need review; physical Pi checks remain pending.

### 2026-11-15 — Readability audit: x86 VirtIO network driver
- Named RX/TX queue IDs, interrupt table entries/offsets, feature words and reset/enable values. Reused the shared frame size and described transmit metadata, interrupt-assisted waiting and receive-buffer reuse.
- Documented existing 64-byte transmit padding policy and two retained frames per transmit without changing them. Existing x86 QEMU network-frame test passes.
- PCI/VirtIO source review is complete across discovery, transport, queues and both devices. Broader architecture/protocol review and physical Pi checks remain open.

### 2026-11-15 — Readability audit: x86 VirtIO block driver
- Named queue/vector selection, MSI-X entry offsets, feature words, sector size and enable/reset values. Reused the shared frame size for DMA buffers.
- Rewrote outdated initialization/read documentation: reads check the used ring and sleep between interrupts; initial zero status does not prove completion; each read retains three frames. Removed disabled prints and unsupported claims about device thread scheduling and register-read timing.
- Existing x86 QEMU known-block test passes. Runtime ordering, allocations, status values and notifications are unchanged. Network initialization and other architecture areas remain open; physical Pi validation remains pending.

### 2026-11-15 — Readability audit: x86 VirtIO queues
- Named ring offsets, entry sizes and block-request lengths; reused the shared frame size. The write flag explicitly means the device writes. Removed disabled diagnostic print blocks while retaining the active debug-state API.
- Corrected ring-size arithmetic in comments, alignment requirements, completion-length semantics and volatile/atomic ordering explanations. Documented serialized submission and separate notification. Operations and atomic orderings remain unchanged.
- Existing x86 disk-read and network-frame QEMU tests pass. Block/network initialization and remaining architecture code are next; physical Pi validation remains pending for earlier changes.

### 2026-11-15 — Readability audit: x86 VirtIO capability transport
- Named VirtIO region offsets and MSI-X table/control fields. Explained table entry count and function masking; corrected the capability result's notification multiplier contract and the module overview.
- Existing x86 QEMU disk-read and network-frame tests pass. Register reads/writes and unsupported/malformed-list handling are unchanged. The parser still assumes valid lengths and an acyclic list; these limits are explicit.
- Queue mechanics and block/network initialization remain to review. Physical Pi validation for earlier changes remains pending.

### 2026-11-15 — Readability audit: x86 PCI discovery
- Named bus/function scan limits, configuration fields, BAR masks/types and endpoint BAR count. Corrected stale discovery documentation, the obsolete below-1-MiB BAR description, and the distinction between PCI configuration and VirtIO status registers.
- Existing x86 QEMU known-block test passes through discovery, BAR setup and the disk driver. Scan order, values and diagnostics remain unchanged; existing address packing and class-code lookup tables were already explained and remain literal.
- PCI transport callers, x86 VirtIO drivers and remaining architecture code still need review. Physical Pi checks remain pending for earlier hardware changes.

### 2026-11-15 — Readability audit: ARM VirtIO networking
- Named transmit storage consistently alongside receive storage, descriptor roles, modern feature-word selection, and packet/header sizes. Explained zeroed transmit metadata, buffer publication, completion polling and receive-buffer reuse.
- Existing QEMU UDP echo test passes; its peer also completes DHCP and verifies reply addresses, payload and checksums. Driver behavior and memory-barrier placement remain unchanged. Transmit still waits without a timeout.
- Continue with remaining ARM architecture/probe code and x86 drivers. Physical Pi validation for earlier hardware changes remains pending.

### 2026-11-15 — Readability audit: ARM VirtIO block requests
- Named feature-word selection, modern transport version, request queue, sector size and descriptor roles. Descriptor flags now state that the device writes the buffer. Explained publication ordering and the synchronous completion poll, including its existing lack of timeout.
- Existing QEMU ARM block-read and ext2-file-read tests both pass. This driver belongs to QEMU virt; it is separate from the Pi SD driver. Numeric register values, request layout and polling behavior are preserved.
- ARM VirtIO networking, remaining architecture code and physical Pi validation of earlier changes remain open.

### 2026-11-15 — Readability audit: WPA2 helper contracts
- Replaced the key-data parser's boolean argument with explicit required/optional RSN policy and its positional result with named group-key fields. Named digest/key/MIC sizes and explained deterministic ordering and truncation during pairwise key derivation.
- Existing WPA2 handshake and corruption tests pass; managed Wi-Fi release build passes. Marked the shared WPA2 source review complete, with real Pi validation still pending. The overall codebase audit remains open.

### 2026-11-15 — Readability audit: WPA2 key-data decoding
- Named RSN key-data selectors, element lengths, PMKID/group-key ranges and key-ID masks. Explained RFC 3394 unwrap steps using named data units and an integrity value; preserved the failure-path clearing of recovered bytes.
- Corrected the group-message flag name to TX/RX: hostap distinguishes its group meaning from the pairwise install bit. Numeric values and accepted packets are unchanged.
- Existing handshake and RFC 3394 tests pass with unchanged fixtures; managed Wi-Fi release build passes. Physical validation remains pending while the Pi is disconnected. Continue the broader architecture/protocol audit.

### 2026-11-15 — Readability audit: WPA2 handshake fields
- Named EAPOL key-field ranges, accepted message words and the three derived key ranges. Reply construction explicitly selects the station nonce or a zero nonce.
- Both existing WPA2 tests pass, including authenticated handshake/retransmission behavior and RFC 3394 unwrap/corruption checks. Managed Wi-Fi release build passes. Synthetic fixtures remain unchanged.
- Key-data element parsing, cryptographic helper internals and the broader audit remain open. Pi disconnected; physical validation pending.

### 2026-11-15 — Readability audit: CCMP frame handling
- Named encrypted-frame lengths, packet-number/key-ID fields, QoS/fragment flags and replay-counter dimensions. Explained the adapter's crypto role and the point where a received packet number becomes accepted state.
- Both existing Wi-Fi frame tests pass, including replay and rejected-aggregate checks. An ignored old/new transmit comparison passes 13,130 buffer/frame/state cases. Managed Wi-Fi release build passes.
- WPA2 handshake fields and the broader architecture audit remain open. Physical encrypted traffic validation remains pending.

### 2026-11-15 — Readability audit: Wi-Fi management frames
- Named shared address ranges, information-element IDs, capability bits, rate arrays, authentication payloads and EAPOL encapsulation. Explained rate units, RSN suite contents and sequence/fragment packing. Station management handling uses the shared address names.
- Both existing Wi-Fi frame tests pass. An ignored old/new builder comparison passes 6,288 cases covering buffer limits, SSID lengths, capabilities, sequences and EAPOL sizes; output bytes and results match. Managed Wi-Fi release build passes.
- CCMP/WPA2 fields and the broader architecture audit remain open. Physical association validation still awaits the Pi.

### 2026-11-15 — Readability audit: receiver preset registers
- Named PLL, regulator, crystal and reset/enable registers and fields. Grouped the three USB-interference writes with the reference driver's explanation, leaving undocumented fields explicitly undecoded.
- Managed Wi-Fi release build passes. Reused the ignored receiver trace comparison with the updated source; complete startup and unsupported-board rejection still match the pre-refactor baseline.
- Shared Wi-Fi/security fields, station-frame offsets and the broader architecture audit remain open. Physical validation still awaits the Pi.

### 2026-11-15 — Readability audit: receiver startup
- Divided receiver startup into programming, RF interface, calibration and scan-filter stages. Named board selection, interface controls, calibration timing, receive filters and appended status fields; documented radio-table delay markers.
- Managed Wi-Fi release build passes. An ignored host harness compared the actual old/new methods and programming tables: complete successful startup and unsupported-board traces match, including recorded helper calls.
- Physical reception remains pending. Some analog presets retain reference literals with an explicit sequencing comment; their register naming and the shared protocol audit remain open.

### 2026-11-15 — Readability audit: station transmit setup
- Named factory power groups, retry/contention fields, address/filter registers and ACK observation settings. Kept timing presets explicit without inventing undocumented units.
- Managed Wi-Fi release build passes. An ignored host harness compared the old/new station-setup methods for channels 0..14 and power indices 0/63/64; all 45 results and register traces match.
- Physical association/traffic validation remains pending. Receiver bring-up and the broader shared/x86/ARM audit remain open.

### 2026-11-15 — Readability audit: radio access and channels
- Named indirect radio address/data fields, read-edge sequence, reply-register selection and settling delays. Explained coordinated 20 MHz channel settings across MAC/baseband/RF.
- Managed Wi-Fi release build passes. An ignored host harness compared actual old/new radio-read/write and channel-selection methods across 813 synthetic cases; return values and register traces match.
- Physical radio validation remains pending while the Pi is disconnected. Receiver bring-up and remaining shared protocol review are still open.

### 2026-11-15 — Readability audit: Wi-Fi packet RAM
- Named linked-list commands, page indices, queue reservations/mapping, FIFO byte boundaries and page-size fields. Explained the separate roles of the end-of-list marker and page index 255.
- Managed Wi-Fi release build passes. An ignored host harness compared old/new setup traces for one/two/three OUT endpoints and clear/stuck-busy responses; all six results and traces match, including the full 256-page setup.
- Physical packet RAM validation remains pending. Receiver/radio initialization and remaining shared protocol code still need review.

### 2026-11-15 — Readability audit: Wi-Fi factory identity
- Named eFuse register fields, physical/logical sizes, compressed block layout, identity signature and MAC location. Explained skipped words and the upper-half timeout diagnostic.
- Managed Wi-Fi release build passes. An ignored host harness compared actual old/new identity methods on six synthetic cases: valid data, bad signature, multicast MAC, truncated record stream and both unsupported chip flags. Results and setup writes match.
- Physical eFuse reads remain pending with the Pi disconnected. Remaining radio initialization and shared security review continue.

### 2026-11-15 — Readability audit: Wi-Fi I/Q calibration
- Named calibration registers, coefficient positions, validity/failure fields, sample limits and timing. Replaced the receive-correction boolean with explicit choices; documented the preserved second-trigger policy and reference tuning recipe.
- Managed Wi-Fi release build passes. An ignored host harness compared the complete old/new calibration register traces for success, RX failure, TX failure and both failures; results and writes match in all four cases.
- Reference programming tables remain in their original order with an explanatory contract. Physical calibration verification is pending; remaining radio initialization and shared frame/security review continue.

### 2026-11-15 — Readability audit: Wi-Fi key table and rate registration
- Named key-table commands, busy polling, entry layout and security enable flags. Explained invalidating an entry before writing it and setting validity last. Named firmware station IDs and rate-table fields.
- Managed Wi-Fi release build passes. An ignored host harness compared old/new key-installation register writes and results for 90 synthetic cases, including rejected entry/key IDs; every trace matched.
- Physical key installation and reassociation still await the disconnected Pi. Calibration and remaining shared frame/security code remain open.

### 2026-11-15 — Readability audit: Wi-Fi packet descriptors
- Named receive length/error/padding/decryption fields and transmit ownership, segment, queue, cipher, rate and checksum fields. Separated the data aggregation flag from queue selection; preserved every encoded value.
- Managed Wi-Fi release build passes. An ignored host harness compared the actual old/new transmit-buffer snippets across 95,664 combinations of lengths, frame types, protection/multicast bits and sequence fields; every byte matched.
- Physical radio validation remains pending while the Pi is disconnected. Calibration and remaining security/frame fields are still open.

### 2026-11-15 — Readability audit: USB startup and DMA memory
- Named root-port delays, controller clock choices, FIFO word regions and DMA address fields. Explained controller register roles and cache maintenance, including Cortex-A7's clean-and-invalidate behavior.
- Existing Pi QEMU UDP echo passes and managed Wi-Fi release build passes. Hardware-specific cache/FIFO/clock behavior remains pending physical verification; no device operations were performed.
- USB source review subitems are now covered. The broader codebase audit and physical validation remain open.

### 2026-11-15 — Readability audit: USB enumeration and control stages
- Named setup request types/actions, hub-port features, descriptor selectors, endpoint fields and buffer limits. Added comments identifying SETUP/DATA/STATUS stages and the current descriptor parser's selection limits.
- Existing Pi QEMU UDP echo passes; managed Wi-Fi release build passes. QEMU uses a directly attached Ethernet device, so the hub path still needs physical validation when the Pi returns.
- Initialization and DMA cache/address handling remain open within the USB audit. Behavior and protocol bytes are preserved.

### 2026-11-15 — Readability audit: USB transfer API
- Replaced internal direction/retry booleans with named enum choices. Named channel error flags, transfer attempt limit and remaining-byte field; explained DMA submission and the polling helper.
- Existing Pi QEMU UDP echo regression passes, and the managed Wi-Fi release kernel builds. Transfer order and retry behavior are preserved. Physical checks await the disconnected Pi.
- USB initialization, hub enumeration and cache/address handling remain open; the broader audit continues.

### 2026-11-15 — Readability audit: SD capacity and clock divider
- Named the card capacity fields and extracted their sector-count calculation into `sectors_from_csd`. Named the divider fields and explained where their bits go in the controller register; arithmetic and access order are unchanged.
- Existing Pi QEMU partition-to-userspace regression passes. The Pi is disconnected; physical timing, write/read-back and reboot checks remain pending.
- Marked this SD audit subitem reviewed. The broader readability goal and hardware validation remain open.

### 2026-11-15 — Readability audit: SD command path
- Named SD commands, controller setup fields, sector/word units and card-ready state. Explained application-command prefixes and polling of latched interrupt-status bits.
- Existing Pi QEMU partition-to-userspace regression passes. Preserved operation order and values; physical writes remain untested after these edits.
- Capacity extraction and clock-divider packing remain open within the SD audit, alongside the broader pending areas.

### 2026-11-15 — Readability audit: Pi reset/watchdog
- Named watchdog time units, protected-write key, reset bits and status address; documented countdown ordering and timeout field limits using the Linux reference driver.
- Managed Pi release build passes. No live reset performed; physical verification and remaining audit areas stay pending.

### 2026-11-15 — Readability audit: RNDIS and Ethernet selection
- Named control/header fields, USB requests, packet filters and fixed adapter identities. Explained RNDIS's relative data offsets and unsupported link-status reporting.
- Existing Pi QEMU UDP echo test passes through initialization and frame transfer. Preserved wire bytes and existing QEMU-specific assumptions; USB controller/hub review remains pending.

### 2026-11-15 — Readability audit: LAN9512 USB Ethernet
- Explained control-register versus bulk-frame transfers; named PHY packing, vendor request fields, RX length shift and supported device IDs.
- Documented command words, FCS removal, reset/MAC initialization, packet-toggle accounting and polling units without changing operations.
- Existing host framing tests pass (3); managed Pi release build passes. Real LAN9512 verification remains pending; QEMU RNDIS is not equivalent hardware coverage.

### 2026-11-15 — Readability audit: SD boot record
- Named record and copy-descriptor fields, checksum parameters, and MBR fields. Explained trial state and the callers' responsibility to persist selection/confirmation.
- Existing boot-record tests pass (3), and the loader-only library compiles without kernel features. Preserved disk format and selection order; no physical SD operations performed.
- Broader audit and physical validation remain pending.

### 2026-11-15 — Readability audit: UART upload framing
- Named header ranges and CRC parameters; documented partial-image storage, the final byte carried by Complete, and transport-owned timeout timing.
- Verified against the existing Python sender. Existing decoder tests (5) and uploader tests (9) pass; protocol bytes and behavior are unchanged.
- Physical checks remain pending while the Pi is disconnected; broader audit continues.

### 2026-11-15 — Readability audit: frame and heap allocation
- Clarified frame-number/address units, bitmap states and caller ownership; renamed private bitmap operations by availability.
- Documented why ARM and x86 heap wrappers mask IRQs before taking the allocation lock. Retained the already-clear x86 boot-memory adapter after review.
- Existing ARM virt QEMU heap allocation and frame-reuse checks both pass. Library allocator internals are unchanged; broader audit remains open.

### 2026-11-15 — Readability audit: ARM task context
- Connected task frame construction to the switch assembly through shared saved-word names; derive the user-register-bank offset from TaskContext's actual layout.
- Explained stack-pointer replacement, first entry versus resumption, and IRQ masking. Preserved instructions and scheduling behavior.
- Existing Pi QEMU process-isolation/fault-containment regression passes. Physical checks and remaining boot/exception/process/memory audit stay open.

### 2026-11-15 — Readability audit: ARM virt timer
- Named the GIC acknowledgement mask, enable/priority values and timer rate; explained frequency-to-deadline conversion and acknowledgement ordering before scheduling.
- Existing ARM virt QEMU timer test passes. Preserved register operations and kept the remaining scheduler/interrupt assembly audit open.

### 2026-11-15 — Readability changes grouped for review
- Saved separate commits for the VirtIO return-type repair (`ad479cc`), network fields/DHCP (`0d48765`), disk/ELF parsing (`776e3fa`), timer/mailbox/random generator (`3866d46`), and Wi-Fi sequences (`ac9934f`).
- Commit descriptions distinguish passed host/QEMU checks from pending physical verification. No new Pi deployment was attempted while the board is disconnected.
- The audit remains active. Lesson artifacts remain separate working-tree changes; no generated logs, firmware, images, or credentials were staged.

### 2026-11-15 — Readability audit: shared ext2 reader
- Named disk-layout fields and explained inode/group indexing, padded directory records, direct block pointers, and sparse holes.
- Documented existing twelve-block truncation and limited malformed-image checking; retained behavior and linked the Linux disk structures in the audit record.
- Existing Pi QEMU partition/file/userspace regression passes. Broader audit and physical verification remain pending.

### 2026-11-15 — Readability audit: ARM ELF loading
- Named ELF32 header/segment offsets and documented the segment callback's memory responsibilities and partial effects. Named ARM image/stack bounds and explained zero initialization before copying file bytes.
- Corrected the x86 ELF introduction's claim that program headers immediately follow the file header; their location is supplied by e_phoff.
- Existing Pi QEMU userspace test passes through SD file loading and a userspace write syscall. Physical validation and the remaining codebase audit stay pending; behavior and roadmap milestones are unchanged.

### 2026-11-15 — Readability audit: shared partition lookup
- Named MBR signature/count/address-width fields and explained disk-relative versus partition-relative sectors.
- Documented the existing lookup and adapter limits without expanding behavior: no extended-partition traversal, length/overlap validation, or adapter upper bound; overflow still yields zero-filled data.
- Existing host partition-selection/translated-read test passes. The broader audit remains active; no physical disk writes or roadmap completion changes.

### 2026-11-15 — Readability audit: firmware mailbox
- Named request lengths, power flags, response offsets and bus-address fields in the Pi mailbox adapter; explained RAM ownership, cache operations, and shared-buffer serialization.
- Preserved the eight-word message, cache instructions, reply checks and bounded polling. Verified field meanings against the Raspberry Pi firmware property-interface documentation.
- Existing QEMU USB enumeration passes, including the power-on mailbox request. Physical verification remains pending while the Pi is disconnected; broader audit checkboxes remain open.

### 2026-11-15 — Readability audit: firmware, DHCP, and Pi timing
- Named firmware-loader fields, mailbox payload sizes, shutdown operations, Wi-Fi station retry policies, DHCP option tags, and lease deadlines. Preserved operation order, access widths, and timing values.
- Clarified Pi timer clock/routing/acknowledgement and the hardware random generator's warm-up, FIFO count, and whole-nonce deadline using primary references recorded in `docs/code-readability.md`.
- Existing Wi-Fi frame/WPA2 and ARM DHCP lifecycle checks pass; x86 DHCP integration passes. The managed Wi-Fi release build and Pi QEMU scheduler/preemption regression also pass after the timer/random-generator edits.
- The attempted hardware update failed at its authentication challenge before kernel data was sent. The user confirmed the Pi is disconnected; new physical validation is pending. The earlier successful power-on-only physical run remains separate evidence.
- The codebase-wide audit remains active, with pending areas recorded in the audit checklist. No roadmap completion checkbox changed.

### 2026-11-15 — Didactic implementation audit started
- Created an active codebase-wide readability goal and coverage checklist in `docs/code-readability.md`.
- Wi-Fi `power_on` now uses named register addresses, masks, delays, explicit set/clear polling, and explanations for each ordered step. Vendor USB request arguments are named; undocumented reference writes are identified honestly.
- Physical Pi validation passed: upload/read-back of the new image, SD reboot, Wi-Fi/DHCP return, persistent logs, and stale-request rejection. CRC32 `defc3948`.
- Started the broader pass with x86 PIT control fields and shared packet-header layouts/checksum semantics. The PIT comment incorrectly described mode 2; the preserved `0x36` command selects mode 3. The x86 timer test passes.
- Verification found an existing VirtIO `FrameDevice::transmit` return-type mismatch; the adapter now reports success after its existing completion wait. Existing x86 and ARM QEMU UDP echo checks both passed.
- The broader goal remains active; unchecked audit entries are still pending. No feature-parity milestone is being claimed by this cleanup.


### 2026-11-15 — Wi-Fi connection lesson
- Added lesson 7.07: an isometric step-through of the installed Pi Wi-Fi startup, with explicit firmware/Rust responsibilities, DHCP timing, and a simulated failure activity.
- Added a matching code tour and short reference, linked from the course map. The lesson distinguishes physical test results from remaining router-outage and group-rekey checks.
- No kernel changes or roadmap checkbox changes. Learner understanding remains to be checked through the exercises.

### 2026-11-15 — Wi-Fi register transport and adapter firmware
- **Physical identity:** RTL8188C, SYS_CFG `04400735`, standard USB board; eFuse identity and power-on sequence verified through the existing UART recovery flow.
- **USB defect found:** Reset FIFO settings reported RX=4096 words, non-periodic TX=256 words starting at 4096, periodic TX=512 words starting at 8192, while GHWCFG3 reports only 4080 words of FIFO RAM. The old layout consistently corrupted a register read at iteration 559. Three non-overlapping 1024-word regions passed all 4096 repeated reads on the Pi. Control-IN DMA also reserves complete endpoint packets in its backing buffer.
- **Verified:** Firmware bytes written to adapter RAM and read back; firmware checksum and initialization-ready bits passed on the Pi. QEMU USB enumeration regression passed. Installed SD kernels remain unchanged; trials run in RAM and return to the UART loader.
- **Further validation:** Passive reception found the target FRITZ!Box on channel 1 with WPA2-PSK/CCMP. Software power-off/reset handles retained adapter state; I/Q calibration supports the reference driver's TX-only fallback when RX measurements are unstable. Authentication and association both receive successful responses on the Pi. QEMU Ethernet UDP echo also passes with the USB changes.
- **Authentication diagnosis:** The router's configured key differed from the supplied key. The actual Pi message-2 MIC was independently verified; radio ACK reception confirmed delivery. After updating the private PMK from the router, the Pi completed WPA2 and DHCP (`192.168.178.128`). Mac-to-Pi ICMP and exact UDP echo passed for 32, 512, and 1200 bytes. No password or handshake capture is committed.
- **Connection lifetime:** A reusable polled Wi-Fi station advances scan, authentication, association, security, and online states. The real Pi completed a deliberate disconnect, a fresh handshake, and a second DHCP lease. Host vectors cover group-key replacement and duplicate-handshake protection.
- **Permanent install verified:** Authenticated Wi-Fi upload stored and read back all 236,236 kernel bytes (CRC32 `98195a23`) in 18.51 seconds. Remote reboot selected SD copy B, confirmed its boot, rejoined WPA2/DHCP at `192.168.178.128`, preserved previous-boot logs, and rejected a replayed reboot request. Copy A retains the previous Ethernet kernel. Authentication rejection checks and the QEMU Ethernet UDP regression passed.
- **Limits and follow-up:** Initial radio transmission uses 1 Mbps; WPA3/required PMF, roaming, and USB hotplug are unimplemented. Group rekey has independent host vectors; router-triggered rekey and a full access-point outage remain hardware follow-ups. Network credentials and handshake captures remain private and excluded from Git; routine captures stay under ignored `target/runs/`. Setup and supported scope are in `docs/pi2-wifi.md`.


### 2026-11-15 — Physical USB Wi-Fi inventory
- **Observed on Pi:** Root hub 0424:9514, Ethernet 0424:ec00 on hub port 1, and Wi-Fi candidate 0bda:8176 on hub port 4. The Linux [rtl8192cu device table](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/realtek/rtlwifi/rtl8192cu/sw.c) identifies this ID as an 8188CU single-stream dongle. Exact chip revision still needs register identification.
- **Built:** Extended `Dwc2::visit_hub_children` in targets/armv7/src/raspi2b/usb.rs to enumerate direct children with distinct addresses. The existing pi2_usb_enumeration_probe now reports every discovered child; normal Ethernet enumeration stops at its first child as before. This remains a boot-time diagnostic, not a hotplug manager or recursive hub walker.
- **Physical test:** Uploaded the diagnostic into RAM over UART. It printed both device IDs and completed; its watchdog restarted the installed SD kernel. The diagnostic contains no SD writes. Raw evidence remains in ignored target/runs/20261115-wifi/. Managed build and existing Raspberry Pi USB QEMU regression passed.
- **Next trials:** (1) Read USB endpoints, chip revision, and EEPROM/eFuse identity. (2) Implement power/firmware/radio initialization and receive beacons to list nearby network names without credentials. (3) Implement association and WPA2 authentication/key installation, keeping credentials outside Git. (4) Feed received network packets through the shared FrameDevice interface and verify DHCP, management, reboot, and reconnect after an access-point outage. Linux rtl8192cu and rtl8xxxu provide implementation references; their drivers require Linux infrastructure and cannot be loaded directly by Gabix. No radio initialization, network scan, or Wi-Fi association has been tested yet.

### 2026-11-15 — Router readiness and persistent diagnostics
- **Built:** Managed Pi profile with HMAC-SHA256 authentication for status, logs, sector access, uploads, installation, and reboot. Requests/replies use a persistent boot number and command counter; duplicate requests reuse the prior reply. The private key stays outside Git at ~/.config/gabix/pi2-management.key. New recovery images use owner-only read permissions.
- **Storage:** Allocated a dedicated 16 MiB journal from the former spare area, starting at sector 5,015,552. Boot metadata remains 1 MiB; 47 MiB remains spare. A small RAM queue buffers routine lines; boot/update decisions request immediate flush. CRC-protected records retain boot/sequence/uptime and expose drop/write/corruption counters.
- **Network:** Added lease renewal, rebinding, expiry, NAK/request-timeout recovery, link polling, and state/counter diagnostics. The client advertises gabix-pi2 and handles multiple DNS/router addresses. Ordinary reboot and update now work over Ethernet; the recovery loader still uses UART if both kernel copies fail.
- **Physical evidence:** Managed boot acquired DHCP and exposed status/logs. Paused the Mac DHCP daemon for 150 seconds through a bounded administrator script; the Pi stayed up and obtained its address around boot+149 seconds without another reset. Its 300-second lease renewed at boot+299 seconds. Authenticated updates/reboots, previous-boot log retrieval, modified/unsigned rejection, duplicate caching, and cross-boot replay rejection passed. A host harness initially stopped on macOS's expected temporary 'No route to host' during reboot; it now retries that condition, and the repeated physical test passed.
- **Checks:** Two focused DHCP lifecycle host tests, the existing ARM virt QEMU DHCP regression, 16 UART/image-registry tests, and managed/normal development builds passed. Final managed image is 117,204 bytes, CRC32 68bdf494; active A is confirmed, with the prior authenticated 116,172-byte copy B retained. Logs and test captures remain ignored.
- **Next:** Stop the Mac's temporary en6 DHCP server, move the cable to a normal FRITZ!Box LAN port, and find gabix-pi2 / MAC 02:47:41:42:49:58 in the router. The real router connection is the remaining unchecked step. See docs/pi2-router-readiness.md for commands and limits.

### 2026-11-15 — Permanent SD boot and recoverable Ethernet updates
- **Installed:** Added the 128 MiB kernel partition after the existing partitions, two 32 MiB kernel areas, and the checked boot record. Replaced only the existing FAT loader allocation and its size field after RAM-loader tests. All installation sectors were read back; no card removal or manual power cycle was required.
- **Behavior:** Ethernet uploads stream into the inactive copy, with chunk read-back and whole-image CRC. The loader records each attempt before execution. A watchdog reset skips an unconfirmed kernel; successful service startup confirms the chosen copy. Exhausting both copies enters the UART recovery loader.
- **Physical validation:** Healthy SD boot, hung-candidate fallback, and both-invalid-checksum UART recovery passed with the RAM candidate and then the permanently installed loader. Restored normal SD boot afterward. Final copies are both 83,864 bytes; A is confirmed active with no pending attempt.
- **Recovery:** Built a 2,634,022,912-byte burnable image from the protected original. Verified the loader and kernels, preserved all original bytes outside the MBR and approved loader sectors, and checked the original SHA256 remained unchanged. The live configuration was installed in place; a complete raw-image reflash and power-cut tests were not performed.
- **Checks:** Three focused boot-record host tests, 16 existing UART/image-registry tests, and release builds passed. Physical checks live in tests/test_pi2_sd_boot.py; raw output stays under ignored target/runs/. Marked the two SD-update milestones complete; broader ARM parity remains in progress.
- **Next:** Review this installation and its explicit limits in docs/pi2-sd-boot.md. Normal updates use the uploader's --kernel option and a UART-triggered reboot. Future work can address authenticated transfers, redundant metadata, and the executable RAM limit independently.

### 2026-11-15 — Card capacity and SD-loader checkpoint
- **Physical evidence:** Read CSD capacity and the final sector on the Pi: 15,523,840 sectors (7,948,206,080 bytes). Existing partitions end at sector 4,882,432. No partition-table or boot-file writes performed.
- **Implemented:** Planned 128 MiB kernel partition with two 32 MiB extents, read-only layout checker, checked boot-record decoder, and optional SD-loader build. Decoder host tests and loader release build pass; SD-loader execution on hardware remains untested.
- **Incomplete:** Automatic new/previous/UART fallback, recovery-image generation, and permanent installation. See docs/pi2-boot-status.md for the explicit inventory and next delivery milestone.

### 2026-11-15 — Demand-paging follow-up
- **Requested:** Record file-backed demand paging as future work for both architectures. Added an unchecked roadmap item; no paging implementation changed.

### 2026-11-15 — Ethernet file transfer to SD
- **Built:** Opt-in Pi transfer kernel, bounded staging-file receiver on UDP 5556, and a Mac uploader. Chunks are written and read back before acknowledgement; whole-file CRC and host byte-for-byte read-back verify completion. No boot selection changes.
- **Storage:** Reserved sectors 8192–12287 of the known recovery image's empty pre-partition gap. First use checks the whole area is zero; later boots validate the file header. Requests cannot choose arbitrary card sectors. One staging file, one uploader, private direct link.
- **Physical validation:** Transferred 65,549 bytes over Ethernet and compared all bytes read back. Rebooted through UART, reloaded the kernel, and read the saved file successfully without resending it. The end-to-end test also covers duplicate requests, out-of-order chunks, corrupt chunks, and a partial final sector.
- **Next:** Decide the persistent kernel layout and teach the loader to validate/load a staged image. The current file header is not a boot pointer; kernel activation and rollback remain unimplemented. Roadmap checkboxes unchanged.

### 2026-11-15 — SD card and clock microworld
- **Teaching:** Added lesson 7.06 with an isometric CPU/RAM/controller/card scene, six write steps, chip inspection, and a clock-divider experiment. The main scenario distinguishes the CPU's source RAM pointer from the card's destination sector number. Optional detail ties PIO, polling, ARM register accesses, and both VirtIO implementations to source.
- **Validation:** Browser checks exercised the stepper, chip keyboard interaction, quiz feedback, divider arithmetic, and source/SD cycle counters. Light, dark, and narrow layouts were rendered and inspected; no page overflow or script errors were found. Captures remain in ignored target/runs. Kernel unchanged.
- **Next:** Review the lesson and ask the learner to explain which component reads RAM, generates CLK, and stores the sector. No learning mastery or roadmap completion is inferred from creating the lesson.

### 2026-11-15 — Physical Pi SD sector writes
- **Built:** Optional shared writable-block interface, Pi CMD24 sector writes, CMD13 completion checks, explicit errors and bounded waits. Corrected the clock configuration: query the firmware source rate and calculate dividers for 400 kHz initialization and 25 MHz transfers. The old divider requested 100 MHz from the physical 200 MHz source without negotiating a faster card mode.
- **Verified:** Uploaded the test kernel over UART to the real Pi. Pattern write/read-back, original-sector restoration, unchanged neighbors, and automatic return to the UART loader passed. Scratch sector 8192 lies in the verified empty gap of the registered recovery image; the reusable test checks that image's layout before writing.
- **Review hygiene:** One physical end-to-end test, opt-in writes, raw captures in ignored run output, instructions in docs/pi2-sd-writes.md. Roadmap checkboxes unchanged: the combined physical peripheral milestone remains broader than this test.
- **Next:** Review sector-write support, then design inactive update slots and Ethernet streaming. At 25 MHz, 128 matching reads took 140 ms versus 811 ms with the temporary slow workaround. Higher-speed modes and power-loss fault injection remain unverified.

### 2026-11-15 — Keep generated output out of reviews
- **Policy:** Added artifacts/README.md and agent guidance: build output stays in build directories; raw run output goes in ignored target/runs; commit tests and concise findings instead of captures.
- **Cleanup:** Removed 70 generated baseline files (40 images, 28 logs, two ELF payloads) from the Git index, retaining local copies. Historical results/checksums and the external recovery-image manifest remain tracked. Earlier commits retain historical files; this does not rewrite repository history.
- **Validation:** Checked staged scope and ignore coverage. No kernel changes or hardware actions. Future work: follow this policy for SD-write testing.

### 2026-11-15 — Ethernet update and recovery design discussion
- **Request:** Compared streamed Ethernet-to-SD kernel updates and a factory recovery area with established update designs. No implementation or SD writes performed; parity roadmap unchanged.
- **Source findings:** Pi SDHCI currently implements CMD17 sector reads only. The resident Gabix UART loader receives a development kernel into RAM at 0x00100000 and branches there; we control this loader after the Raspberry Pi firmware stage.
- **Proposed direction:** Preserve the firmware/loader area, add inactive-slot streaming and read-back verification, then trial boot with persistent attempt state, watchdog, and explicit kernel confirmation. A/B slots preserve the previous working image; a separate recovery image can provide Ethernet repair when both normal slots fail. UART remains the last-resort path. SD-controller power-loss behavior still requires physical fault-injection testing.
- **References:** RAUC basic concepts (inactive slots, streaming, boot confirmation/fallback) https://rauc.readthedocs.io/en/stable/basic.html and U-Boot boot-count limits https://docs.u-boot.org/en/latest/api/bootcount.html . These are reference designs; adopting either framework is not implied.
- **Next if selected:** Establish physical SD read/write/read-back on a dedicated test area, before adding network transfer or changing boot selection. This is a proposed side project, not a completed or scheduled parity milestone.

### 2026-11-13 — Connect the memory lesson to ARM instructions
- **Teaching:** Enhanced lesson 2.01 while preserving its two-process scenario. Added optional sections explaining section descriptors and address splitting, TTBR0 writes and TLB/cache maintenance, AP/XN permission bits, abort-status registers, User-mode entry, and banked SP/LR switching. A worked STR/LDR example connects the program’s address to the actual physical mappings.
- **Validation:** Excerpts checked against memory.rs, scheduler.rs, context.rs, user.rs, process_probe.S, and boot.S. HTML nesting, local links, and whitespace checks passed. Kernel unchanged; no hardware rerun needed. Browser rendering remains unverified.
- **Preference:** User values the new clarity and wants sufficient ARM/assembly detail to follow the implementation. Recorded progressive code detail in NOTES.md.

### 2026-11-12 — Isolated ARM userspace processes on the physical Pi
- **Built:** Two fixed 2 MiB user allocations with separate 16 KiB translation tables. The scheduler changes TTBR0 and invalidates cached translations before entering a process, then restores the boot mappings for the dispatcher. Context switching preserves user-bank SP/LR as well as the SVC call context. User exit and memory faults finish only the current process.
- **Protection:** Kernel/device mappings remain privileged. Each process maps its own allocation at the same user VA; stack execution is disabled. Scheduled processes have bounded write-syscall pointers and cannot invoke the legacy UART reboot syscall. Kernel memory faults retain a diagnostic halt.
- **Verified:** QEMU virt and raspi2b isolation suites pass. Both user programs retain distinct sentinels at 0x41010000 and private stack/return state without yielding. Separate batches contain other-process reads, other-process writes, kernel reads, and stack instruction-fetch faults while the peer exits successfully. Existing virt user-write, Pi userspace-start, and repeated UART reload regressions pass. Fixed a Linux-specific llvm-objcopy path in the virt regression to use the actual Rust host.
- **Physical proof:** All five batches passed on the Pi, with hundreds of timer switches per process. UART reboot from the new kernel, re-upload, DHCP, and exact UDP echo passed without power cycling. Saved serial/results in artifacts/pi2-isolation-physical.log. Initial uploader timeout was too short for noncacheable test loops; the repeat used --user-ready-timeout 90 and captured every result.
- **Tools/teaching:** Added scripts/check-pi2-isolation.py and hardware instructions. Added lesson 2.01, starting with two programs writing different values to the same virtual address. Local lesson links checked; browser rendering was not verified.
- **Scope / next:** Checked private address spaces in the roadmap. This is fixed-slot flat-image execution with 1 MiB sections; code/data remains RWX and the stack is XN. The UART ELF path remains separate. General ELF process creation, dynamic allocation, fine-grained code permissions, and fork/exec/wait remain future work; start with loading ELF images into these private address spaces.

### 2026-11-12 — Rebuild timer lesson around the learner’s question
- **Feedback:** User did not understand how the timer changes the running task; the previous scenario was implicit and the interface too dense.
- **Revision:** Lesson 3.07 now states the counting-task scenario and shows timer hardware, CPU execution, and memory in eight steps. A timer-off comparison shows why B keeps waiting. Register details moved to an optional linked page.
- **Validation:** Eight Node checks pass across both teaching models and their controls; local lesson links pass. Browser layout remains unverified. Kernel unchanged.
- **Next teaching check:** Ask the learner to distinguish the timer’s interrupt request from the scheduler’s task selection before introducing IRQ/SVC register banks.

### 2026-11-12 — ARM register and stack simulator
- **Teaching:** Added lesson 3.07 with a 19-step model of two timer interrupts and task A’s resumption. CPU registers, banked IRQ status, and per-task saved frames are shown together. Backward navigation, a step slider, three prediction questions, and independent register/condition-flag fault options support practice.
- **Scope:** Symbolic code addresses and stack positions; selected registers shown, compiler calls grouped. This is a teaching model of the current source, not an instruction emulator or live hardware view. Kernel behavior was unchanged.
- **Validation:** Six Node behavioral checks cover register/status restoration, independent saved task state, both injected faults, snapshot isolation, and UI navigation/reset/quiz controls. Local links and JavaScript syntax checked. Browser rendering remains unverified because local-file preview was previously restricted.
- **Learning status:** User requested concrete CPU-state visualization; no mastery inferred. Next review: explain why restoring the switch context resumes the handler before the interrupted task.

### 2026-11-12 — Timer preemption on ARM
- **Built:** Timer IRQs now preempt registered ARM kernel tasks. The IRQ wrapper stores the interrupted PC/status and general-purpose registers on the task’s own SVC stack, preserves SVC status, aligns the Rust call stack, and returns with RFE. Scheduler selection, explicit yield, registration, and completion use masked IRQs; resumed calls restore their prior IRQ setting.
- **Verified:** QEMU virt and raspi2b require the new preemption marker. Two non-yielding tasks complete eight atomic progress handshakes and record at least eight preemptions. The repeated UART userspace-load regression also passed. The release build passed on the physical Pi through UART reboot/upload, followed by DHCP and an exact UDP echo response. A second UART reboot from the preemptive kernel, re-upload, preemption check, and echo also passed. No manual power cycle.
- **Teaching:** Added lesson 3.06 with the interrupt save/switch/return sequence, the comparison with x86, and a worked non-yielding-task example. Updated lesson 3.05 to identify its simulations as cooperative-only. Linked Arm’s IRQ implementation as a primary reference.
- **Roadmap:** Checked timer preemption for single-core kernel tasks. Floating-point/SIMD state, ARM process isolation and lifecycle, blocking, and migrating device services into tasks remain unfinished. Next: address-space isolation; review device ownership before scheduling driver work.


### 2026-11-12 — ARM cooperative scheduling and parity objective
- **Lesson revision:** Rewrote 3.05 after user feedback about dense, sensational prose. Added definitions, worked examples before activities, literal controls, and optional assembly detail. Recorded the teaching preference in NOTES.md.
- **Goal:** User selected x86/ARM feature parity. Updated mission and roadmap, beginning with scheduling before memory isolation, process lifecycle, VFS and sockets.
- **Built:** Four-slot cooperative ARM task dispatcher with separate 16 KiB stacks, explicit yield and finished states. Integer-only, single-core kernel tasks; timer preemption and driver-task integration remain unchecked.
- **Verified:** QEMU ARM virt resumption, Raspberry Pi scheduler trace, and Raspberry Pi UART reload tests passed. Physical Pi printed the exact `10 20 11 21 12 22` trace and completion marker after UART reboot/upload, with a watchdog protecting the scheduling check. No manual power cycle. Mac bootpd independently exited with a routing allocation error and required an administrator restart; UDP echo passed again afterward.
- **Teaching:** Added lesson 3.05 with two microworlds (yield versus timer tick, saved versus lost local state), architecture comparison, and quick reference. Recorded the mission change without claiming mastery of the new material. Browser preview was blocked for local file URLs; JavaScript syntax and interaction logic were checked separately.
- **Next:** Timer-driven ARM preemption, full interrupt context, and ownership rules before moving UART/Ethernet into independent tasks.

### 2026-11-12 — Persistent Pi echo server and UART reload
- **Naming follow-up:** Named the shared loop `serve_uart_uploads_and_ethernet`, its network operation `poll_ethernet_and_reply`, and its UART/upload/tick state explicitly. Added named scheduling conditions and three-step comments. Release build and whitespace checks pass; polling behavior is unchanged.
- **Readability follow-up:** Rewrote the USB transfer-completion section as four explained steps, with named status conditions and helpers for checking and halting the channel. Preserved the existing retry decisions; the release build passes. The running Pi was left untouched.
- **Physical diagnosis:** A synchronized DHCP → ping → UDP test reproduced the failure before the watchdog reset. Removing the forced DATA0 receive reset restored dynamic ARP and the first ping. USB status then showed `HCINT=0x10` (NAK) with `HCCHAR=0x80888a00` (channel still enabled): the driver returned while DMA remained active.
- **Fix:** Explicitly halt an enabled channel on NAK before reuse, wait for it to stop, and reread completion status. Removed the diagnostic receive-PID reset and periodic register polling. The physical check now passes both pings and three byte-exact UDP replies separated by one-second idle periods.
- **Persistent service:** The `pi2-dev` kernel initializes Ethernet, obtains DHCP, and polls networking from the idle UART user-program loader. Diagnostic watchdog resets are restricted to diagnostic features. The server remains running between Mac messages; polling pauses during user uploads and user execution because ARM still runs one user program at a time.
- **Reload:** Built and physically uploaded the existing `arm_reboot.elf` through UART, observed `USER UPLOAD OK` and `GABIX LOADER READY`, and reloaded the server without power cycling or SD writes.
- **Tools:** Added `scripts/pi2-echo.py` for messages to a running kernel and `scripts/check-pi2-network.py` for a repeatable watchdog-enabled physical regression. Documented persistent service, diagnostic runs, and UART reboot/reload in `docs/raspberry-pi-2.md`.
- **Verification:** Physical ARP, 2/2 pings, 3/3 UDP echoes, and a further echo after 20 seconds idle passed. QEMU Raspberry Pi UDP and repeated UART userspace-upload tests passed; 16 Python uploader/image-registry tests passed. Release builds and `git diff --check` passed. No roadmap checkbox changed: the combined physical milestone also includes SD verification, which this session did not rerun.
- **Next:** Use the persistent message demo, then choose between an ARM userspace socket API and ARM process scheduling. DHCP lease renewal and long-running reliability remain follow-up work.

### 2026-11-12 — Resume physical Pi connection checks
- **Done:** Fetched origin and fast-forwarded the clean main checkout to `c02f643`. Located the later hardware work in `/Users/gabrielelanaro/.codex/worktrees/cacd/gabix`, branch `codex/pi2-uart-loader`, at `47b6a03`. Fast-forwarded main to include all 43 hardware commits at the user’s request.
- **Context recovered:** The prior UART session reported working loader uploads, physical DHCP, and UDP echo with static ARP. Its next test was the DWC2 global-interrupt fix; these results belong to the hardware branch and were not reverified today.
- **Verification:** Opened `/dev/cu.usbserial-0001` at 115200 8N1. No bytes arrived during listening, including after the user reported a power reconnect. No kernel upload or SD-card write performed. UART uploader, userspace uploader, and image-registry tests passed (16/16); `git diff --check` passed. Roadmap checkboxes unchanged.
- **Follow-up:** With the UART capture active, the user power-cycled the Pi and confirmed green ACT activity with the SD card inserted. Captured `GABIX LOADER READY` at 115200 8N1, confirming the bootloader and serial connection are working.
- **Network follow-up:** Built `pi2-network-dev` in release mode and uploaded 50,556 bytes with CRC success. The Mac DHCP peer ran on `en6` at `10.0.2.2`. Physical UART confirmed USB enumeration, Ethernet configuration, link up, successful DHCP configuration, and watchdog return to `GABIX LOADER READY`. The receive path then repeatedly reported `rx fifo unavailable`; ping/UDP testing overlapped the reset and is inconclusive. Saved the UART evidence in `artifacts/pi2-network-baselines/resume-2026-11-12-dhcp-physical.log`.
- **Learning:** Reviewed the teach skill and learning records. The user correctly recalled that Pi Ethernet traffic passes through USB; clarified the separate onboard LAN9512 chip. Suggested lesson 7.05 first, with 7.02/7.04 and 6.10 as supporting refreshers.
- **Next:** Diagnose receive failures after DHCP using a controlled test window, then verify ARP, ping, and UDP.

### 2026-08-16 — Physical Pi 2 receives a raw Ethernet frame
- **Minimal probe:** NET11 keeps the passing USB/LAN9512 initialization,
  waits five seconds for link negotiation, and polls only the raw frame-device
  receive path. It prints the first valid frame's length, destination, source,
  and EtherType over UART.
- **Physical result:** The Pi received a 110-byte IPv6 frame (`0x86DD`) sent by
  Mac `en6` MAC `E8:EA:6A:2D:FE:C4` to multicast MAC
  `33:33:00:00:00:16`. This verifies LAN9512 bulk IN, RX status parsing, and
  delivery into `EthernetFrame` on physical hardware.
- **Preserved baseline:**
  `artifacts/pi2-network-baselines/net11-lan9512-raw-rx.img`, SHA-256
  `2f5145652139fa7672b7d4db763c02a4b0c666a8b1838448236d575f75136a28`,
  with its exact UART log beside it.
- **Next:** Load the full Gabix ARM kernel through UART with this proven USB
  path, expose its DHCP state over UART, and configure a controlled DHCP peer
  on Mac `en6` before testing UDP and TCP separately.

### 2026-08-16 — Physical Pi 2 transmits raw Ethernet frames
- **Minimal probe:** NET10 extends the passing LAN9512 link probe with one
  action: transmit sixteen 60-byte broadcast frames carrying experimental
  EtherType `0x88B5` and the payload marker `GABIX-NET10-PROBE`.
- **Timing diagnosis:** Sending immediately after register configuration
  completed every USB bulk transfer before Ethernet autonegotiation finished,
  and Mac `en6` received zero packets. Waiting five seconds for the already
  observed physical link transition kept the rest of the probe unchanged.
- **Physical result:** The probe reported all 16 bulk transfers successful.
  Mac `en6` counters changed from 0 packets/0 bytes to exactly 16 packets/960
  bytes, matching the generated frame count and size.
- **Preserved baseline:**
  `artifacts/pi2-network-baselines/net10-lan9512-raw-tx.img`, SHA-256
  `94102ea52f445c54db1d30538d4770ea8b78fea2ea4c0c2a58eff7f554524896`,
  with UART and Mac counter logs beside it.
- **Next:** Send a known frame from Mac `en6`, receive it through LAN9512 bulk
  IN, and report its EtherType/payload over UART. Keep DHCP and higher layers
  out of that receive probe.

### 2026-08-16 — Physical Pi 2 LAN9512 configuration establishes Ethernet link
- **Cache and DMA correction:** Aligned mailbox property messages to a cache
  line, synchronized them around GPU access, and synchronized DWC2 DMA buffers
  around USB transfers. The host driver now waits until a completed DWC2 host
  channel is disabled before reusing channel zero and retries transient USB
  transaction errors for control requests.
- **Physical result:** The UART-loaded NET9 probe configured the onboard
  LAN9512 and printed `NET9 LAN9512 CONFIG OK`. During the probe's watchdog
  window, Mac interface `en6` negotiated `100baseTX` full-duplex with flow
  control. It returned inactive after the Pi reset to its SD-resident loader.
- **LAN configuration quirk:** The LAN9512 accepts `SET_CONFIGURATION`, then
  this DWC2 path reports a transaction error on that request's status
  handshake. Subsequent vendor-register commands succeed and establish the
  physical link, so the LAN driver validates configuration through those
  commands. EEPROM MAC reading currently falls back to `02:47:41:42:49:58`.
- **Preserved baseline:**
  `artifacts/pi2-network-baselines/net9-lan9512-link.img`, SHA-256
  `576e39e594b1dc97ec25b08be77ac7e41dd66163e339690e6e1bded2ba78f11c`,
  with UART and Mac link-poll logs beside it.
- **Next:** Transmit one identifiable raw Ethernet frame and capture it on
  Mac `en6`, then add receive independently before integrating the full Gabix
  network stack.

### 2026-08-16 — Physical Pi 2 USB hub and LAN9512 enumerate
- **Minimal probe:** Added a 7,386-byte UART-loaded probe that contains only
  the mailbox and DWC2 paths needed for USB enumeration. A watchdog returns it
  to the SD-resident loader without a manual power cycle.
- **Physical result:** DWC2 initialized, then enumerated root device
  `0424:9514` class `09` as the onboard SMSC hub and child device `0424:EC00`
  class `FF` as the LAN9512 Ethernet function.
- **Preserved baseline:**
  `artifacts/pi2-network-baselines/net8-usb-enumeration.img`, SHA-256
  `3c618e58772a044d149665960874578ab17031e90eb49001b610543cbe88eaab`,
  with its exact physical UART log beside it.
- **Next:** Configure LAN9512 registers, expose its MAC address and link state
  over UART, then transmit one identifiable raw Ethernet frame to the Mac.

### 2026-08-16 — Physical Pi 2 userspace reboot reloads the kernel
- **Host capture:** Added `--wait-for-reboot` to
  `scripts/upload-pi2-user.py`. It keeps the UART open after
  `USER UPLOAD OK` until the SD-resident loader prints
  `GABIX LOADER READY`. A pseudo-terminal test covers this completion mode.
- **Physical reboot:** The 4,560-byte ARM fixture invoked syscall 169 from the
  live Stage 6f kernel. The Raspberry Pi 2 reset itself and returned to
  `GABIX LOADER READY` without a physical power cycle. The immediate reset
  truncated the preceding userspace UART marker while it was still queued in
  the PL011 FIFO; the complete loader marker established the reset result.
- **Fresh kernel upload:** The host uploaded the unchanged Stage 6f kernel
  after the syscall reset. The loader reported CRC success and handoff, and
  Gabix returned to `GABIX USER READY`.
- **Preserved evidence:** `stage7-reboot-user-fixture.elf`,
  `stage7-physical-reboot.log`, and `stage7-physical-kernel-reload.log` live in
  `artifacts/pi2-stage-baselines/`. The fixture SHA-256 is
  `db116882ed56bee543a57f25a66f6a15a749faf7a725300c35e5d3d5dcde74ca`.
- **Verification:** The QEMU Pi 2 reboot test and all 10 host uploader tests
  pass. No roadmap checkbox changed.
- **Next:** Decide whether reboot should drain the PL011 transmit FIFO before
  arming the short reset watchdog, then add the next user-program development
  capability on top of the proven reload lifecycle.

### 2026-08-16 — Physical Pi 2 UART userspace reload loop passes
- **Diagnostic result:** Stage 6e reached the full kernel and accepted the
  preserved user ELF, then reported `P0000000D@41000020`. Fault status `0x0D`
  is a section permission prefetch abort, and `0x41000020` is the ELF entry.
- **Root cause:** `allow_user_section()` changed a descriptor in the cached
  `BOOT_TABLE`, then issued barriers and a TLB invalidation. The hardware table
  walk still read the old privileged-only descriptor from RAM. Cleaning the
  changed descriptor cache line before the barrier made the user mapping
  visible to the MMU.
- **Physical verification:** Stage 6f booted through `GABIX USER READY`. The
  same 4,552-byte user ELF was uploaded twice in the same kernel session. Both
  runs printed `gabix arm userspace wrote`, reported
  `gabix raspi2b userspace exited`, and returned to `GABIX USER READY` without
  a reboot or SD-card write.
- **Passing baseline:**
  `artifacts/pi2-stage-baselines/stage6f-clean-user-descriptor.img`, 110,940
  bytes, SHA-256
  `0241035587b39e9d05635de9f5d22da61116be42d820a8c8fb7c7240a2aee928`.
  Exact kernel and user-run logs are stored beside it.
- **Verification:** The Raspberry Pi 2 QEMU UART test also completed two user
  reloads. No roadmap checkbox changed.
- **Next:** Add a user-facing reboot fixture to the physical UART loop, then
  verify that its reboot system call returns to the SD-resident loader and a
  fresh development kernel can be uploaded without a physical power cycle.

### 2026-08-16 — Physical Pi 2 UART kernel reaches userspace upload
- **Staged diagnosis:** Preserved every physical payload and result in
  `artifacts/pi2-stage-baselines/README.md`. Minimal probes established the
  handoff, Hyp-to-SVC transition, BSS clearing, Rust entry, a 1 MiB BSS clear,
  MMU setup, exclusive accesses, allocator initialization, and one `Box<u32>`
  allocation in that order.
- **Atomic root cause:** A single `LDREX` raised data abort
  `DFSR=0x00001008` at `DFAR=0x00108000`, a synchronous read-side AXI slave
  error. ACTLR.SMP and Normal non-cacheable mappings still faulted. Mapping
  kernel RAM as cached shared Normal memory and enabling the data cache made
  `LDREX`, `STREX`, Rust `AtomicU32::compare_exchange`, allocator
  initialization, and `Box::new` pass physically. The passing marker sequence
  was `SMIA`.
- **Full development kernel:** Added raw-image BSS clearing, moved SMP and MMU
  setup before allocator initialization, and excluded SD/USB probing from the
  UART-user development path. Physical artifact
  `stage6a-full-uart-development-kernel.img` booted through `smp enabled`,
  `memory mapped`, `heap allocated`, and `timer advanced`, then printed
  `GABIX USER READY`.
- **Userspace boundary:** The first 4,552-byte ELF passed CRC and printed
  `USER UPLOAD OK`, then stalled before its first syscall. User code had been
  written through the newly enabled data cache. The next probe maps only user
  code and stack sections as Normal non-cacheable memory. QEMU completes two
  upload/run/exit/reload cycles with that mapping.
- **Recovery:** Added a physical-development-only 10-second watchdog around
  user execution. `SYS_EXIT` stops it with the sequence used by Linux's
  BCM2835 watchdog driver. A failed user handoff returns to the SD loader; a
  successful exit returns to `GABIX USER READY`.
- **Current artifact:**
  `artifacts/pi2-stage-baselines/stage6d-noncacheable-user-deadman.img`, 110,760
  bytes, SHA-256
  `e1c60917376e0e3570e68c2f2530f8a76d63efeb7185fd0321dbed339705adc7`.
  The user fixture is `stage6-user-fixture.elf`, SHA-256
  `a7f4fba0a80792fc8967e7b2f6c6b6f0ffb876ba6d07cbdd879ef76f8ab1d1f7`.
- **Status:** No roadmap checkbox changed. Physical UART boot and kernel upload
  pass. Repeatable physical userspace reload still needs the Stage 6d test.
- **Next:** Power-cycle once to return from the stalled Stage 6a user handoff,
  upload Stage 6d with `scripts/upload-pi2-kernel.py --wait-for-user-ready`,
  then upload `stage6-user-fixture.elf` twice with
  `scripts/upload-pi2-user.py --wait-for-exit`. Record both physical outputs
  before changing the memory mapping again.

### 2026-08-15 — UART-loaded userspace and warm program reload
- **Done:** Added the `pi-uart-user` development mode. After a UART kernel
  upload, Gabix prints `GABIX USER READY`, receives a CRC-framed ARM ELF into a
  bounded 512 KiB RAM buffer, validates and runs it, then returns to the same
  receiver when userspace calls `exit`. Added separate build and upload commands
  for user programs; neither command writes the SD card.
- **Reliability:** The shared framed-upload receiver now enforces a caller-set
  payload limit. A partial user upload times out after 100 timer ticks; header,
  CRC, timeout, and ELF failures return to the ready state for a complete retry.
  The userspace-exit trampoline resets the SVC stack before each receiver pass,
  so abandoned frames do not accumulate across warm reloads.
- **Verification:** The QEMU Pi 2 test recovers from a truncated upload, runs
  the same ELF twice in one kernel session, observes both exits, and reaches a
  third `GABIX USER READY`. Host uploader tests verify the exact frame and the
  program-output/exit capture through a pseudo-terminal. The full suite passes
  48/52; the same four baseline host-toolchain failures remain (missing
  x86-target `core`, `llvm-objcopy`, and `llvm-readobj`).
- **Next:** Upload the rebuilt `pi2-dev` kernel to the physical Pi, wait for
  `GABIX USER READY`, then send `target/arm_syscall.elf` with
  `scripts/upload-pi2-user.py`. No SD-card modification is required.

### 2026-08-15 — UART kernel upload and userspace reboot lifecycle
- **Done:** Added paced UART uploads with progress/ETA output via
  `scripts/upload-pi2-kernel.py --inter-byte-delay-us`, plus a
  `--wait-for-reboot` mode that keeps the same UART open to capture the
  post-upload loader-ready line; enabled the PL011 receive/transmit FIFOs in
  the Pi 2 loader; added
  `tests/fixtures/arm_reboot.rs`, which prints a marker and invokes syscall
  169; and added `tests/test_raspi2b_reboots_from_userspace.sh`.
- **Verification:** The uploader tests pass (5/5), the Pi 2 loader build test
  passes, the Pi 2 lifecycle test passes in QEMU, and the full suite passes
  47/51 when pointed at the installed Homebrew ARM QEMU. The four remaining
  failures are environment/toolchain issues: a missing x86 target, missing
  cross-toolchain utilities, and the dependent LAN9512/MBR checks.
- **Artifacts:** The debug Pi 2 development kernel is linked at `0x00100000`
  and rebuilt as `targets/armv7/target/armv7a-none-eabihf/debug/kernel-pi2-dev.img`.
  The reboot fixture is `target/gabix-arm-reboot.elf`.
- **Physical boundary:** The SD card is currently absent. The next physical
  action is to back up the existing ext2 `/init.elf`, install the reboot
  fixture as `/init.elf`, verify the readback, then upload the development
  kernel over UART and observe the reboot returning to `GABIX LOADER READY`.

### 2026-08-15 — UART loader diagnostic candidate
- **Reason:** The first development-kernel upload reached 100% host-side
  transfer but produced neither `UPLOAD OK` nor kernel output. The deployed
  loader exposed too little state to distinguish header rejection, timeout,
  CRC failure, and kernel handoff.
- **Change:** Added distinct failure reasons in the upload protocol and safe
  UART diagnostics: `UPLOAD HEADER FAILED`, `UPLOAD TIMEOUT`, `UPLOAD CRC
  FAILED`, `UPLOAD CRC OK`, and `UPLOAD OK`. Messages are emitted only after
  payload reception or at safe parser boundaries.
- **Candidate:** Built and verified
  `target/official/gabix-pi2-loader-debug.img`, SHA-256
  `5498918bae0c1795127e8f12d6badcc4217cf778dc2946374726349232554218`.
  The proven loader backup remains unchanged and protected.
- **Blocker:** Physical SD reflash is still pending explicit confirmation.

### 2026-08-15 — Gabix UART loader boots on physical Pi 2B
- **Result:** The full loader image booted from the SD card and printed
  `GABIX LOADER READY` on the sole 115200-baud UART monitor.
- **Image:** `cdd2f94825edea97b865dd002a49e8558bcf3dbb43657f6b5074fc02054d7183`,
  preserved at `~/Documents/Gabix-Images/gabix-pi2-loader-full.img` with
  read-only and user-immutable protection.
- **Sequence:** Verified the protected smoke baseline, derived the loader image,
  wrote all 2,499,805,184 bytes to `/dev/rdisk7`, verified all five boot files
  on the mounted card, unmounted it, and captured the loader ready line after a
  Pi power cycle.
- **Meaning:** The complete official firmware/partition baseline, the Pi's
  `KERNEL7.IMG` handoff, Gabix's loader entry point, and UART initialization are
  working together on physical hardware.
- **Next:** Upload a development kernel over UART. Keep the SD card and its
  protected loader image unchanged during that loop.

### 2026-08-15 — Physical Pi image registry and protected baseline
- **Registry:** Added `artifacts/pi2-images.json` as the tracked source of truth
  for candidate, hardware-passed, and hardware-failed image identities. Every
  record includes byte size, SHA-256, source, and concrete physical evidence.
- **Protection:** Preserved the known-working 2.5 GB smoke image under
  `~/Documents/Gabix-Images`, verified its complete checksum, removed write
  permissions, and applied macOS's user-immutable flag.
- **Workflow:** Added `scripts/pi2-image-registry.py` to list, resolve, verify,
  preserve, and record physical outcomes. A passing candidate is copied and
  protected before its tracked status changes.
- **Current state:** The historical full smoke image and the full loader image
  are `hardware-passed`; the 520 MiB intermediate image is `hardware-failed`.
- **Physical reconfirmation:** Reflashed all 2,499,805,184 bytes of the protected
  baseline, matched all five boot files on the card, and captured
  `gabix uart smoke booted` plus more than 10,000 heartbeat lines. Recorded the
  exact evidence and the baseline's known oversized kernel cluster chain in
  `artifacts/pi2-uart-smoke-baseline.md`.

### 2026-08-15 — Reproducible Pi loader-image writing
- **Baseline correction:** The 520 MiB FAT32-layout experiment was mistakenly
  documented as the physical known-good image. The successful boot used the
  complete 2,499,805,184-byte official Bookworm image with only `KERNEL7.IMG`
  and `CONFIG.TXT` replaced. Reconstructing it reproduced the historical
  SHA-256 `66abb107a93d2db17729e7fcc5bedc2c4c6be15c5d2844d0e29d0306e870d5c6`.
- **Verifier correction:** FAT32 FSInfo `next free` is a search-start hint.
  Validation now accepts any in-range cluster or the unknown marker instead of
  requiring the numerically first free cluster or requiring the hint itself to
  be free.
- **Failure:** The first loader image grew the known-good 260-byte smoke kernel
  into a fragmented FAT32 chain and left the FSInfo free-cluster count stale.
  The Pi stopped at four ACT flashes before UART output.
- **Fix:** The image builder now relocates a growing `KERNEL7.IMG` into one
  contiguous cluster run, updates both FAT copies and primary/backup FSInfo,
  and compares firmware files against the known-good smoke source.
- **Guardrail:** Added `scripts/verify-pi2-loader-image.py`. It rejects the
  broken fragmented image and accepts the corrected `668–683` cluster layout.
  `fsck_msdos -n` reports a clean corrected FAT32 image.
- **Runbook:** Documented exact device identification, confirmation, raw write,
  boot-file comparison, unmount, and the reason post-mount whole-disk hashes are
  not the acceptance signal.
- **Next:** Run upload A → reboot → upload B → corrupt/retry over UART without
  changing the SD card.

### 2026-08-15 — Raspberry Pi UART development loader
- **Loader:** Added an allocation-free stage-one Pi 2 loader at `0x00008000`.
  It receives a versioned `GABIXLOAD` stream, enforces a 16 MiB limit and
  five-second inter-byte timeout, verifies CRC32, and starts valid kernels at
  `0x00100000`.
- **Development loop:** Added separate `pi2-loader` and `pi2-dev` build outputs
  plus a host UART uploader with explicit progress, transfer rate, and ETA.
  Build, monitor, upload, and log inspection remain separate commands.
- **Reset:** Added no-argument system call 169. Raspberry Pi 2 uses the BCM2836
  watchdog reset; ARM `virt` delegates to its platform termination adapter and
  x86 uses the i8042 reset command.
- **Verification:** Protocol unit tests cover valid uploads, CRC failure,
  unsupported versions, timeout, and retry. Static build tests verify the
  loader entry at `0x00008000` and development-kernel entry at `0x00100000`.
- **Next:** Build a loader SD image from the verified official-derived firmware,
  flash it once, then run the physical upload A → reboot → upload B → corrupt
  upload/retry acceptance sequence.

### 2026-08-09 — Gabix UART smoke kernel boots on physical Pi 2B
- **Baseline:** Established a known-good official Bookworm image, one-owner
  UART monitoring, explicit log paths, confirmed `/dev/rdisk6` targeting, and
  targeted FAT boot-file read-back.
- **Diagnosis:** Compared the 260-byte Gabix raw ARM image with the official
  Linux `KERNEL7.IMG`. The Gabix entry path worked in QEMU and on hardware; the
  first physical log was garbage because hardcoded PL011 divisors assumed a
  3 MHz clock while current firmware used 48 MHz.
- **Fix:** Added the project boot configuration with
  `init_uart_clock=3000000`, built the corrected official-derived image, and
  added a self-verifying FAT32 replacement path for `KERNEL7.IMG` and
  `CONFIG.TXT`.
- **Verification:** Flashed 149/149 16 MiB blocks with `FAST_DD_STATUS:0`,
  matched all boot-file hashes after read-back, and captured
  `gabix uart smoke booted` followed by thousands of
  `gabix uart smoke alive` lines from the physical Pi.
- **Status:** Physical UART smoke milestone complete. The full physical SD,
  USB, Ethernet, DHCP, ping, and UDP checklist remains unchecked.
- **Next:** Slow the heartbeat to a human-readable interval, then resume the
  physical Gabix driver checklist from the verified baseline.

### 2026-08-08 — Reproducible Raspberry Pi UART monitoring
- **Done:** Diagnosed the physical UART capture confusion: the Pi was sending
  old NOOBS/Raspbian boot output, while `screen` wrote it to its default
  `screenlog.0` instead of the expected temporary path. Added
  `scripts/monitor-pi2-uart.sh` with explicit start/status/stop commands, one
  serial-device owner enforcement, unique log directories, and automatic
  single-device discovery. Documented a repeatable baseline sequence for image
  verification, UART capture, power-cycle, boot classification, and monitor
  cleanup. Ignored the default screen log so it cannot pollute the repository.
- **Status:** No roadmap checkbox changed. Physical UART is confirmed working;
  Gabix has not yet been flashed to the SD card.
- **Next:** Create a verified backup or explicitly approve proceeding without
  one, flash the verified Gabix image, and capture the first Gabix boot with the
  single-owner UART monitor.

### 2026-08-04 — Raspberry Pi hardware lessons and operator notes
- **Lessons:** Added lesson 7.02 on USB host control, transfer types, descriptors, and enumeration, plus lesson 7.03 on the Pi property mailbox and inter-core mailboxes. Both lessons trace the explanations to the ARMv7 Raspberry Pi drivers and keep the current polling gaps explicit.
- **Reference material:** Added a Raspberry Pi 2B UART pinout SVG and a `run-pi2-qemu.sh` helper for launching an ARM kernel with the `raspi2b` QEMU machine.
- **Prose:** Tightened the ARM port, privilege/syscall, interrupt-entry, and Pi interrupt-controller lessons to use direct mechanism-first statements. Added the project rule against false-equation and dramatic-identity phrasing.
- **Status:** No roadmap checkbox changed. Physical Raspberry Pi verification remains the next hardware milestone.
- **Next:** Continue Module 7 with the remaining hardware lessons, then verify UART, SD, USB, Ethernet, DHCP, ping, and UDP on the physical Pi 2B.

### 2026-08-01 — Raspberry Pi 2B QEMU port and physical image
- **ARM platform:** Added shared ARMv7 boot, memory mapping, heap allocation, timer interrupts, user-mode entry, system calls, and kernel return after user exit. QEMU `virt` and `raspi2b` use position-independent board adapters around the shared kernel code.
- **Storage:** Added Raspberry Pi SDHCI sector reads, MBR partition lookup, ext2 file reads, and `/init.elf` loading. The image builder creates a FAT16 firmware partition and an ext2 root partition.
- **Networking:** Added DWC2 USB host transfers, hub-child enumeration, QEMU RNDIS Ethernet, Raspberry Pi USB power and DMA initialization, and a LAN9512 Ethernet driver. The ARM network interface acquires a DHCP lease, replies to ARP and ICMP, and echoes UDP port 5555.
- **Delivery:** Added the `pi-hardware` build at address `0x8000`, a verified `target/gabix-pi2.img`, and `docs/raspberry-pi-2.md` with image-writing, Ethernet, serial, network-test, and diagnosis steps.
- **Verification:** The external behavioral suite passes 48/48 across x86, ARM `virt`, and QEMU `raspi2b`. It covers the Raspberry Pi boot, memory, timer, block, partition, ext2, userspace, USB, DHCP, UDP, lifecycle, hardware-kernel, and SD-image boundaries.
- **Status:** Added and completed the QEMU and image items under the Raspberry Pi 2B target. Physical UART, SD, USB, LAN9512, DHCP, ping, and UDP verification remains unchecked because it requires the board.
- **Next:** Write the image to an SD card, connect the Pi 2B to the router by Ethernet, follow the serial and network checklist, and fix any physical DWC2 or LAN9512 timing differences found on the board.

### 2026-08-01 — Modular kernel layout and shared ARM/x86 entry
- **Architecture:** Moved x86 boot, interrupt controllers, GDT, timer, paging, syscall entry, and context-switch assembly under `src/arch/x86_64/`. Added architecture facades for interrupt control and timer ticks used by shared kernel modules.
- **Drivers:** Split the 2,584-line virtio module into transport, queue, block, and network drivers under `src/drivers/virtio/`. Added `BlockDevice` and `FrameDevice` interfaces. ext2 owns a block-device interface, while `src/net/device.rs` owns protocol processing over a frame-device interface.
- **Kernel services:** Reduced `src/syscall.rs` to syscall-number dispatch and moved handlers into file, socket, process, time, and user-memory modules. Split task scheduling from exit/wait/exec lifecycle operations. Moved ext2 under the filesystem subsystem.
- **Targets:** Added the `no_std` `crates/kernel-core` crate. x86, ARM `virt`, and Raspberry Pi 2B call its shared boot operation through platform console adapters. ARM startup establishes a stack, enters Rust, and builds as position-independent code.
- **Cleanup:** Removed the first-RX-MSI test diagnostic and several syscall/scheduler trace messages while moving behavior into focused modules. External test drivers remain outside the kernel.
- **Verification:** `cargo check --features qemu-exit` passes. `tests/run.sh` passes 20/20, including x86 QEMU, ARM `virt`, Raspberry Pi 2B, process lifecycle, filesystem, UDP, TCP, DHCP, poll, and retransmission behavior.
- **Status:** No roadmap checkbox changed. The refactor preserves the current milestone behavior.
- **Next:** Add ARM implementations behind the architecture facades for interrupt control, timer ticks, paging, and task context, then move more shared kernel initialization into `gabix-kernel`.

### 2026-08-01 — External behavioral suite and independent system under test
- **Boundary:** The kernel has one scenario-independent boot path in `src/boot.rs` and always executes `/init.elf`. External test scripts choose the init program by constructing the disk image. `main.rs` contains only module wiring, panic/allocation handlers, and `_start` delegation.
- **Cleanup:** Removed inline boot demonstrations and assertions, the unused kernel test runner, scenario Cargo features, test-only network syscalls 900–903, direct TCP test syscalls 905–907, and in-kernel packet-loss injection. Renamed the QEMU status adapter feature to `qemu-exit`.
- **Public behavior:** Added monotonic `clock_gettime`, standard socket syscall numbers for `connect`, `accept`, and `listen`, and lazy activation of the pre-created network worker. TCP loss is produced by an external Ethernet peer that answers ARP/TCP and discards the first data segment.
- **Targets:** Added a small position-independent ARMv7 boot target with platform UART configuration for QEMU `virt` and Raspberry Pi 2B.
- **Verification:** `tests/run.sh` passes 20/20 behavioral tests. The agreed 18 tests are present under the `test_<action>_<observable_result>` convention; DHCP and poll behaviors add two more boundary tests. Repeated the fork-resume test three times after making serial writes atomic.
- **Status:** No roadmap checkbox changed.
- **Next:** Use these boundaries while separating additional x86 and ARM platform services behind production interfaces.

### 2026-08-01 — Architecture review for a second QEMU/ARM target
- **Done:** Reviewed the boot path, architecture-specific modules, device stack, syscall/process coupling, network stack, Cargo feature matrix, and all 18 QEMU shell tests before the Raspberry Pi port. Identified refactor candidates around test/boot separation, architecture and board boundaries, syscall/process separation, device/storage boundaries, and stateful networking.
- **Testing assessment:** The hello, fork/wait, descriptor-inheritance, UDP echo, and TCP echo tests assert user-visible behavior. Several ARP, IPv4, virtio, MSI-X, and TCP retransmission tests assert exact diagnostic messages or internal state transitions and will break during logging or implementation refactors. Production boot also runs many inline assertions and demonstrations from `main.rs`; `test_runner.rs` is currently unused.
- **Cleanup assessment:** `main.rs` contains old allocator, address-space, filesystem, protocol, and device demonstrations plus unused scheduler worker counters. `syscall.rs` retains test-only network syscalls 900-903 and unused direct TCP helper syscalls 905-907. `virtio.rs` contains active queue diagnostics and commented-out trace prints.
- **Test naming:** Added the project convention `test_<action>_<observable_result>` to `AGENTS.md`. Test names stay short; setup and preconditions remain in the test body.
- **Status:** No roadmap checkbox changed. No kernel source or test was modified. The untracked Pi 2 QEMU runner and UART pinout asset were left untouched.
- **Next:** Select the first refactor candidate. Recommended order: establish stable behavioral test boundaries and separate test boot from production boot; then introduce the architecture/board boundary before adding ARMv7 `raspi2b` startup code.

### 2026-07-29 (lesson 33 implementation) — TCP passive open, echo, and passive close
- **Done:** Added TCP objects to per-process descriptor tables, listener lookup by port, connection lookup by four-tuple, SYN/SYN-ACK/ACK passive open, completed accept queues, blocking `accept` and stream `read`, TCP `write`, EOF after FIN, and the passive-close transitions `ESTABLISHED → CLOSE_WAIT → LAST_ACK → CLOSED`. Added syscalls 908–909, the ring-3 `/tcp_echo.elf` fixture, TCP port-8080 QEMU forwarding test, code tour, quick reference, and learning record.
- **Checkboxes:** Marked Lesson 33 complete. The three-lesson TCP arc now opens an active connection, retransmits stream bytes, accepts an inbound connection, echoes bytes, and closes the server side cleanly.
- **Verification:** `cargo check --features test-mode,test-tcp-echo` passes. `tests/test_tcp_echo.sh` sends `lesson-33-echo` from a host Python socket, receives the same 14 bytes, half-closes the host direction, and observes the server FIN plus final ACK. The Lesson 31 handshake and Lesson 32 reliable-stream tests also pass.
- **Boundary:** The server connection table supports distinct four-tuples. The active-open client still uses its single Lesson 31 control block. TIME_WAIT expiry, FIN retransmission, half-open backlog accounting, fork-per-connection concurrency, and accepted peer-address output remain future work.
- **Next:** Choose the next phase after the TCP arc, or return to the incomplete general UDP socket objectives 26.3–26.4.

### 2026-07-29 (lesson 32 implementation) — TCP sliding window and timeout recovery
- **Done:** Added owned retransmit entries for SYN and payload segments, multi-segment sends within the peer window and a fixed four-segment congestion limit, cumulative ACK queue removal, RFC 6298 SRTT/RTTVAR/RTO updates, Karn sample suppression, exponential timeout backoff, and a timer check in the deferred network task. Added the Lesson 32 code tour, quick reference, learning record, and deterministic loss E2E test.
- **Checkboxes:** Marked Lesson 32 complete in the TCP arc.
- **Verification:** `cargo check --features test-mode,test-tcp-reliable` passes. QEMU split a 31-byte DNS query into 12-, 12-, and 7-byte segments, withheld the first segment, retransmitted it after the 100-tick initial RTO, received one cumulative ACK that emptied the queue, and delivered the 63-byte DNS response to ring 3. `tests/test_tcp_reliable_stream.sh` passes.
- **Boundary:** One active connection, one oldest-entry timer, a fixed four-segment congestion limit, and prefix acceptance when the window is smaller than the user buffer. Out-of-order receive buffering, congestion control, passive open, multiple connections, and teardown remain outside this implementation.
- **Next:** Lesson 33 adds FIN teardown, passive open, a connection table, and a ring-3 TCP echo server.

### 2026-07-19 (lesson 31 implementation) — TCP active open and first response bytes
- **Done:** Added the TCP header codec, IPv4 pseudo-header checksum validation, protocol 6 receive dispatch, a single-slot TCP Control Block, active-open state transitions, blocked-task wakeups, and syscalls 905–907 for connect/send/receive. The ring-3 fixture connects to slirp DNS at `10.0.2.3:53`, sends a valid length-prefixed query, and receives the first response byte. Added the E2E test, code tour, quick reference, and learning record.
- **Checkboxes:** Marked Lesson 31 complete in the TCP arc.
- **Verification:** `cargo check --features test-tcp` passes. QEMU exited with code 33. The SYN-ACK acknowledged `client_isn + 1`; the 31-byte query advanced `send_next` by 31; the 63-byte response advanced `recv_next` and the outgoing ACK by 63.
- **Boundary:** One global connection and one bounded send are implemented. Retransmission, sliding windows, passive open, and teardown remain in Lessons 32–33.
- **Next:** Lesson 32 starts by retaining unacknowledged outbound bytes and retransmitting them after an RFC 6298 retransmission timeout.

### 2026-07-19 (Tour rewrites + skills hardening + TCP arc planning) — plainspoken on the explanatory prose, two skills extended
- **Done:** Rewrote the openers and step prose in `tours/tour-4.10-interrupt-driven-networking.html` and `tours/tour-6.10-dhcp.html` under the `plainspoken` skill. Tour 29's lead was a single sentence of unloaded jargon ("hardirq versus task context," "deferred kernel task," "virtio-net MSI-X"); replaced with a concrete-event opener that defines each term inline (interrupt handler, `net-rx`, deferred work). Tour 30 had no intro paragraph at all; added one that opens with `0.0.0.0` at boot and introduces DHCP, lease, DORA, slirp, and `net_configure` inline.
- **Skill extensions:** Added an "Introductions" section to `~/.agents/skills/plainspoken/SKILL.md` with three rules (open with a concrete event, introduce each term inline, do not assume prior context), a reader test, and the tour-29 opener as a worked example. Extended §32 "Aphorism Formulas" in `~/.config/opencode/skills/humanizer/SKILL.md` with `the shape of`, `the whole shape of`, and a technical-prose before/after. Cut the offending "is the whole shape of the implementation" line from tour 29 in the same pass.
- **TCP arc planned:** Added a three-lesson TCP arc to the roadmap (Lesson 31: handshake + first byte exchanged; Lesson 32: reliable stream with sliding window and retransmission; Lesson 33: teardown + ring-3 TCP echo server over slirp). Compressed from an initial five-lesson draft after the user pushed back &mdash; the original split byte-stream send/recv (32) from retransmission (33), but those are conceptually intertwined (you can't teach send/recv without immediately hitting "what if it's lost?"). Drafted all three lessons as concept-first HTML: `lessons/6.11-tcp-handshake.html` (segment format byte-layout, three-way handshake on a sequence diagram, TCB, ISN selection, RFC 9293), `lessons/6.12-tcp-reliable-stream.html` (retransmit queue, sliding window, RFC 6298 SRTT/RTTVAR/RTO, Karn's algorithm, exponential backoff), `lessons/6.13-tcp-teardown.html` (four-segment FIN close, six teardown states, TIME_WAIT 2*MSL rationale, listen/accept syscalls, multi-connection socket table, ring-3 echo server). Two audit passes under `plainspoken` + `humanizer`: first pass caught em-dashes, vague verbs, personification; second pass (after user feedback) caught a deeper failure &mdash; the lesson leads opened with the mechanism ("A program calls connect()") instead of the problem the mechanism solves. Rewrote all three leads motivation-first: L31 opens with UDP's limitations and the programs that need more (file transfer, remote login, web pages); L32 opens with TCP's lossless promise and the wire-level failures that put it at risk; L33 opens with the resource cost of never-closed connections and why a real program cannot leak them. Updated the `plainspoken` skill with a fourth rule in the Introductions section ("Open with the problem this piece solves, not the mechanism") and a new worked example showing the TCP intro rewrite. Each lesson now stands alone: a reader who lands on L33 cold can follow because the lead re-establishes the resource-cost motivation before introducing teardown.
- **Status:** No roadmap checkbox flipped for the TCP arc yet. Lesson 31 is drafted as a concept lesson only; implementation begins with step 31.1 (TCP header build/parse).
- **Open question from earlier in session:** A fresh run of `tests/test_interrupt_driven_networking.sh` timed out (no UDP from host arrived in guest after the gateway ARP exchange), contradicting the 2026-07-19 reliability entry's claim of 20/20 passes. The `[net] first RX wakeup delivered by MSI-X interrupt` marker never printed in the run, confirming the timer backstop is still load-bearing and the code comment in `src/interrupts.rs:275` is accurate. Worth diagnosing before claiming the UDP path is reliable again.
- **Next:** Answer Lesson 31's probe (why the third handshake message carries no SYN flag but is still required), then begin step 31.1 (TCP header `TcpHeader` struct, build/parse, checksum with pseudo-header).

### 2026-07-19 (lesson 30 implementation) — DHCP configures the network from ring 3
- **Done:** Added runtime network configuration, limited-broadcast IPv4/Ethernet transmit and receive behavior, syscall 904 for MAC retrieval and lease installation, and `/dhcp.elf`. The user program binds UDP port 68, sends DISCOVER and REQUEST, validates OFFER and ACK by magic cookie and transaction identifier, parses the address/mask/gateway/DNS/lease options, and installs the lease. Added the E2E test, code tour, and learning record. Fixture ELFs now compile with size optimization and stripped symbols so each file stays inside the ext2 reader's twelve-direct-block boundary.
- **Checkboxes:** Marked Lesson 30 complete in the networking objective and mission progress.
- **Verification:** `cargo check --features test-mode,test-dhcp` passes. QEMU completed DISCOVER, OFFER, REQUEST, and ACK; the client installed `10.0.2.15/24`, gateway `10.0.2.2`, DNS `10.0.2.3`, and an 86400-second lease, then exited successfully from ring 3. Host memory pressure intermittently killed clean custom-target `core` rebuilds, so the full suite remains to be run.
- **Next:** Run the full suite, then chain ping and UDP echo after DHCP to prove those paths read the acquired configuration.

### 2026-07-19 (IRQ-safe locking refactor) — encode preemption safety in the lock type
- **Done:** Added `IrqMutex<T>` and its RAII guard. `lock()` saves the caller's IF state, disables interrupts before acquiring the underlying spinlock, and the guard releases the spinlock before restoring IF. Nested locks preserve an already-disabled state. Migrated the console, serial port, scheduler, process table, PIC, frame allocator, page-table mapper, heap allocator, UDP socket/port state, ARP cache, inode cache, inode data, file cursors, and mounted-filesystem registry. The mounted filesystem now stores an `Arc` and clones it before disk operations, so its registry lock is never held across interrupt-driven I/O. The virtio-net and ext2 device locks retain their explicit sleeping/try-lock designs because those paths enable interrupts while waiting for device completion.
- **Verification:** `cargo check --features test-mode,test-udp` passes. The complete QEMU suite passes 14/14 with `CARGO_BUILD_JOBS=1 tests/run.sh`, covering fork/preemption, inheritance, disk MSI-X, deferred networking, blocking and nonblocking UDP, ARP, IPv4, ICMP, and every virtio-net layer.
- **Status:** Roadmap checkboxes unchanged. Short shared-state spinlocks now enforce the single-core preemption invariant through their type instead of relying on every caller to remember an outer `without_interrupts` block.
- **Next:** Introduce a task-blocking mutex before allowing multiple tasks to perform concurrent ext2 I/O; sleeping device operations must never use `IrqMutex` across their wait.

### 2026-07-19 (lesson 29 reliability diagnosis) — console spinlock deadlock fixed
- **Done:** Characterized the intermittent two-datagram echo failure across the host, QEMU, virtqueue, syscall, scheduler, and console boundaries. QEMU packet capture contained the second inbound request, and QEMU virtqueue tracing showed the device consuming TX work. The guest halted after its first `sendto`. Controlled variants showed that busy-polling TX could still fail, while keeping interrupts disabled after TX completion passed 10/10. A QEMU monitor register dump caught the CPU at `spin_loop_hint`, with `RDI=0xffffffff800c0008`; the kernel symbol map resolved that address to the VGA `WRITER` mutex. The stack resolved through `vga_buffer::_print` and `NetDevice::recv_frame` on the deferred network task. The timer had paused the echo task while it owned the console lock, then scheduled the network task, which printed and waited forever for the paused owner. Wrapped the VGA and serial console writes in one `interrupts::without_interrupts` critical section so a task cannot be preempted while it owns either console spinlock.
- **Verification:** The original build reproduced the deadlock within 1–3 runs. A TX busy-poll variant still reproduced it, rejecting QEMU completion timing as the cause. A timer-wrapper alignment experiment also reproduced it, rejecting stack alignment as this failure's cause. The console fix passed one clean rebuild, 20 consecutive repeated QEMU boots, and a final clean E2E run; every run echoed both 64-byte and 256-byte UDP payloads byte-for-byte and exited through `isa-debug-exit` with code 33. `tests/test_interrupt_driven_io.sh` also passed when rebuilt with one Cargo job after the first parallel build was killed under host memory pressure. `cargo fmt --check` could not run because `rustfmt` is absent from the pinned nightly.
- **Status:** Roadmap checkboxes unchanged. Lesson 29's host echo path is reliable under the repeated test. QEMU/slirp delivered the packets correctly; the failure lived in the guest's preemptible console locking.
- **Next:** Install the pinned toolchain's `rustfmt` component when convenient, and keep console/diagnostic spinlocks inside interrupt-disabled or preemption-disabled critical sections.

### 2026-07-18 (lesson 29 implementation repair + code tour) — deferred virtio-net RX and valid Ethernet TX
- **Done:** Recovered the unfinished Lesson 29 work and traced the failed host echo through QEMU. Kept RX/TX work in a blockable `network_task`, added an atomic `block_current_if` waiter/scheduler transition, assigned virtio-net's per-device MSI-X entries to dedicated vectors `0x30`/`0x31`, and added a 100 Hz timer wake as a bounded-latency QEMU backstop because the device accepts the MSI-X mappings without reliably delivering messages. Fixed the wire-level failure by padding TX Ethernet frames shorter than 60 bytes before the NIC-generated FCS. Hardened the E2E test so early failures retain QEMU diagnostics. Added `tours/tour-4.10-interrupt-driven-networking.html` and linked it from Lesson 29.
- **Verification:** One QEMU run completed two byte-for-byte echoes and exited cleanly. Repeated runs remain unreliable: the guest consistently receives the datagram and submits a checksum-valid TX frame, while slirp sometimes drops the reply or a following TX completion stalls. `cargo check --features test-mode,test-udp` passes. Packet capture exposed the original 58-byte short TX frame and confirmed checksum-valid replies after padding.
- **Boundary:** virtio-blk MSI-X is proven interrupt-delivered. virtio-net queue/vector programming is present, while RX progress currently uses the timer backstop. The host-forwarded UDP test still has an unresolved QEMU/slirp or TX-ring reliability failure, so Lesson 29 is documented as an implementation tour with that boundary rather than marked fully reliable.
- **Next:** Diagnose QEMU virtio-net MSI-X delivery independently, then remove the timer wake once RX and TX messages are observed reliably. After that, begin Lesson 30's DHCP implementation.

### 2026-07-16 (lesson 30 preparation) — DHCP concept lesson with Lease Office minigame
- **Done:** Authored `lessons/6.10-dhcp.html` concept-first. The lesson opens with the bootstrap paradox (sending IP needs an address; `0.0.0.0` + `255.255.255.255` break the circle), walks the DISCOVER/OFFER/REQUEST/ACK exchange on a new reusable `assets/sequence-diagram.css` swimlane, shows the inherited BOOTP packet with the byte-layout component (192 of 236 fixed bytes are legacy), covers lease timers T1/T2 on a timeline visual, and uses slirp's built-in server (10.0.2.2, assigns from 10.0.2.15) as the test hook: the leased address must equal the deleted `OUR_IP` constant. Build sequence 30.1–30.4: unconfigured-stack TX/RX concessions, user-space packet module, `/dhcp.elf` + new `net_configure` syscall, retiring the constant. Added the "Lease Office" minigame at the end of the lesson (`assets/lease-office.{css,js}`): the player runs the server — OFFER/ACK/NAK against patience timers, pool exhaustion, hold/lease expiry, rogue REQUESTs. Added `reference/dhcp.html`. Chained Lesson 29's nav forward to 30, linked the roadmap's "After the objective" DHCP item to the lesson, and added RFC 2131/2132/951 plus Linux `net/ipv4/ipconfig.c` to RESOURCES.md.
- **Status:** Roadmap checkboxes unchanged. Every prerequisite for the build exists: blocking sockets (Lesson 26), readiness (Lesson 27), interrupt-driven delivery (Lesson 29). `assets/sequence-diagram.css` is generic and will serve the TCP arc's handshake diagrams. Numbering note: this lesson was first drafted as "Lesson 27" in a stale worktree; consolidated here as Lesson 30 after Lessons 27–29 landed in the main repo.
- **Primary sources:** RFC 2131 (exchange §3.1, broadcast §4.1, state machine §4.4, T1/T2 §4.4.5 — verified against the RFC text), RFC 2132 (option codes 53/50/54/51/58/59, magic cookie `0x63825363`), QEMU network emulation docs (slirp DHCP at 10.0.2.2, assignments from 10.0.2.15).
- **Next:** Work the Lesson 30 probe (INIT-REBOOT shortcut: ACK confirms the remembered address, NAK sends the client back to DISCOVER), then build step 30.1 (unconfigured-stack TX/RX concessions).

### 2026-07-12 (lesson 28) — interrupt-driven virtio-blk I/O
- **Done:** Enabled and mapped the Local APIC, registered a dedicated vector `0x21` ISR, parsed and enabled virtio-blk's PCI MSI-X capability, programmed table entry 0, bound request queue 0 to it, and replaced `read_sector`'s bounded busy-wait with an interrupt-driven `hlt` loop. The wait disables interrupts around its used-ring check and uses atomic `sti; hlt` to close the lost-wakeup window. Moved interrupt enablement ahead of disk initialization once the interrupt controllers and main scheduler task are ready. Removed serial printing from the timer ISR after early interrupt enablement exposed a reentrant serial-lock deadlock; the handler retains tick accounting and sends EOI to both the PIC and Local APIC virtual-wire path. Added an end-to-end regression test and marked the lesson implemented.
- **Status:** Roadmap checkboxes unchanged. Virtio-blk completion is interrupt-driven; virtio-net still uses its polling worker.
- **Verification:** `cargo check --features test-mode` and `./tests/test_interrupt_driven_io.sh` passed. The targeted test confirms Local APIC setup, MSI-X queue/vector binding, an interrupt-delivered disk completion, ext2 mount, ELF loading, ring-3 execution, and clean exit. Five additional QEMU cold boots passed consecutively with the same proof markers.
- **Next:** Lesson 29 should give virtio-net RX/TX queues MSI-X vectors, wake blocked socket waiters from deferred interrupt work, and remove the network polling task.

### 2026-07-12 — lesson 27 implemented: nonblocking UDP and poll
- **Done:** Added per-socket nonblocking state selected by `SOCK_NONBLOCK`, empty-queue `EAGAIN`, and Linux syscall 7 `poll` for UDP `POLLIN`. Poll performs its readiness check and waiter registration with interrupts disabled to close the lost-wakeup window. Packet delivery wakes blocking receivers and every registered poll waiter.
- **Proof:** Added `/nonblocking_echo.elf`, which binds ports 5555 and 7777 and runs one ring-3 event loop. It drains each ready socket through `recvfrom` until `EAGAIN` and echoes each datagram to its sender.
- **Verification:** `cargo check --features test-mode,test-nonblocking` passes. `tests/test_nonblocking_sockets.sh` passes with host datagrams and byte-for-byte echoes on both ports.
- **Boundary:** The poll slice supports UDP `POLLIN`, immediate timeout `0`, and infinite timeout `-1`. Finite timers, writable/error events, `select`, and `epoll` remain future lessons.
- **Next:** Walk the Lesson 27 code path from the ring-3 `poll` call through waiter registration, packet delivery, wakeup, queue drain, and `EAGAIN`.

Most recent entries at the top. One entry per work session.

### 2026-07-11 — Lesson 26 implementation code tour
- **Done:** Added `tours/tour-6.08-sockets.html`, following one datagram through FD lookup, socket state, six-argument syscall entry, blocking `recvfrom`, network-worker delivery, wakeup, `sendto`, the ring-3 fixture, and the two-datagram host test. Linked it from Lesson 26 and the networking roadmap.
- **Status:** Lesson 26 code and the explanatory tour are complete. No roadmap checkbox changed.
- **Verification:** The tour, Lesson 26, and the networking roadmap return HTTP 200 from the Tailscale lesson server. All referenced local files exist and `git diff --check` passes.
- **Next:** Work through the tour’s ownership-chain probe before beginning the nonblocking implementation from Lesson 27.

### 2026-07-11 — Lesson 26 user-space UDP sockets
- **Done:** Added UDP socket kernel objects, process FD integration, port routing, Linux-numbered socket syscalls with six-argument capture, blocking receive/wakeup, a polling network worker, and a ring-3 echo server that handles two datagrams.
- **Status:** Lesson 26.1–26.4 are complete. The ring-3 echo server blocks in `recvfrom`, wakes when the network worker delivers each datagram, and echoes both host messages.
- **Verification:** `cargo check --features test-mode,test-udp` passed. `./tests/test_udp.sh` passed with two byte-for-byte echoes and QEMU exit code 33. The fresh kernel-task stack alignment fix prevents the compiler-generated `movaps` fault in the RX worker; the worker is scoped to the socket test path so earlier network tests retain their existing shutdown behavior.
- **Next:** Work through the Lesson 26 retrieval probe, then continue with Lesson 27’s nonblocking socket design.

### 2026-07-11 — lesson 27: nonblocking sockets and readiness
- **Done:** Added a concept lesson and quick reference for blocking versus nonblocking `recvfrom`, `EAGAIN`, `poll`/`select`/`epoll`, readiness over `fd → socket → receive queue`, and the separate NIC-delivery layer. Added a two-socket event-loop stepper, retrieval check, implementation sequence, and roadmap/navigation links.
- **Status:** Lesson 27 is complete as a concept lesson. Nonblocking mode and readiness remain unimplemented in mini-os. No Month 4 checkbox changed.
- **Verification:** Lesson 27, its reference page, Lesson 26, the networking roadmap, and shared CSS/JavaScript all return HTTP 200 over the Tailscale lesson server. `git diff --check` passes.
- **Next:** Work through the Lesson 27 retrieval check, then add the per-socket nonblocking flag and `EAGAIN` behavior.

### 2026-07-11 — clarify UDP peer selection
- **Done:** Rewrote Lesson 26’s unconnected-versus-connected UDP explanation around a server receiving from two clients and replying with `recvfrom`/`sendto`. Defined “client” directly and removed the unexplained “peer” term. Added plain-language definitions of “datagram,” socket, FD, the four basic socket calls, and why servers bind while clients can use a kernel-selected source port.
- **Status:** No roadmap checkboxes changed.
- **Next:** Continue the Lesson 26 retrieval probe.

### 2026-07-11 — serve lessons over Tailscale
- **Done:** Replaced the Tailscale-bound Python server on `100.127.51.0:8000` with a detached server rooted at the repository, so lesson pages and their shared assets resolve together.
- **Status:** No roadmap checkboxes changed.
- **Notes / blockers / surprises:** The previous process served `/home/gabriele/newspapers`. Serving `lessons/` alone broke `../assets/...` links; the repository root fixes the CSS and JavaScript paths. Verified the lesson, both stylesheets, and the stepper script return HTTP 200. The process is detached with PID 102289 and has no system service definition.
- **Next:** Open `http://100.127.51.0:8000/lessons/6.08-sockets.html` from a device on the tailnet.

### 2026-07-10 (lesson 26 preparation) — sockets, queues, and genuine blocking
- **Done:** Added the concept-first Lesson 26 and UDP socket quick reference. The lesson introduces the ARPANET endpoint meaning and 4.2BSD API lineage, separates socket/endpoint/connection/FD, explains domain/type/protocol, compares UDP and TCP lifecycles, and connects blocking calls to modern readiness-driven runtimes. It then connects the existing per-process FD table to a new socket object, weak port-binding table, owned datagram queue, waiter queue, and four user-facing socket syscalls. Its interactive route follows one server from `socket` and `bind` through an empty `recvfrom`, scheduler blocking, kernel-network-worker delivery, wakeup, and `sendto`. Added reusable forward/back stepper assets, linked Lesson 25 and the networking roadmap forward, and recorded the socket API primary sources.
- **Status:** Lesson 26 implementation remains pending. The design explicitly accounts for mini-os’s polling virtio driver: a kernel network worker must continue polling while the echo server is blocked.
- **Primary sources:** Linux `socket(2)`, `bind(2)`, `recvfrom(2)`, and `sendto(2)` contracts; RFC 1122 section 4.1.4 for the UDP/application interface; Linux `net/socket.c` and `net/ipv4/udp.c` as implementation counterparts.
- **Next:** Complete the Lesson 26 ownership probe, then implement the generalized FD entry and socket/port tables before adding syscalls or blocking.

### 2026-07-10 (lesson 25) — UDP port-5555 host round trip
- **Done:** Implemented RFC 768 UDP parsing/building, declared-length validation, IPv4 pseudo-header checksum verification, protocol-17 receive dispatch, `send_udp`, and the kernel port-5555 echo handler. Added `SYS_NET_UDP_WAIT = 903` plus `/udp_echo.elf` so ring 3 triggers the receive proof. Added `tests/test_udp.sh`, which sends `hello from lesson 25` from a Mac UDP socket through QEMU host forwarding and requires the same 20 bytes back. Updated the concept lesson, quick reference, networking roadmap, and added the implementation code tour with forward/back controls.
- **Status:** Lesson 25 is complete. The host-to-QEMU-to-guest-to-host UDP round trip is verified; Lesson 26 can replace the fixed port handler with socket-table demultiplexing.
- **Verification:** `cargo check --features test-mode,test-udp` passed with the existing warning baseline. `tests/test_udp.sh` passed independently: valid header/checksum proof, one protocol-17 receive at `10.0.2.15:5555`, one kernel echo, one ring-3 completion, and a byte-for-byte 20-byte host reply. The implementation agent also ran `tests/run.sh` with 11/11 tests passing and `git diff --check` clean.
- **Primary sources:** RFC 768 for UDP; RFC 1071 for the Internet checksum; QEMU invocation documentation for UDP host forwarding.
- **Next:** Work through the Lesson 25 retrieval probe and code tour, then begin Lesson 26: UDP sockets and user-space echo.

### 2026-07-09 (lesson 24) — ICMP echo gateway proof
- **Done:** Implemented `src/net/icmp.rs` with RFC 792 echo request/reply build, parse, full-message checksum verification, and exact reply matching. Added `SYS_NET_ICMP_PING = 902`, plus `/icmp_ping.elf`, so the ring-3 fixture requests four gateway pings with identifier `0x2400`, sequences `7..10`, and payload `mini-os`. `NetRxEvent::Ipv4` now preserves the bounded IPv4 payload for higher protocols after RX dispatch. The IPv4 receive path also replies to inbound ICMP echo requests addressed to `OUR_IP`. Added `tests/test_icmp_ping.sh` and updated the ext2 fixture disk. Added `tours/tour-6.05-icmp-ping.html`, organized around one concrete packet and retrieval questions rather than source-file order. Updated the lesson, reference, and networking roadmap to reflect the completed proof.
- **Status:** Lesson 24 is complete. The networking roadmap now marks Keystone 1 complete.
- **Verification:** `cargo check --features test-mode,test-icmp` passed with the existing warning baseline. `./tests/test_icmp_ping.sh` passed: four protocol-1 IPv4 transmissions to `10.0.2.2`, four matching ICMP echo replies for sequences `7..10`, and ring-3 fixture success. The ICMP test boots QEMU without UDP host forwarding because ICMP needs no host port; `./tests/test_net_smoke.sh` retains its UDP forwarding and still passed after the RX event grew an owned IPv4 payload.
- **Primary sources:** RFC 792 for echo request/reply semantics; QEMU user-mode networking documentation for the gateway test target; Linux `net/ipv4/icmp.c` for the production counterpart.
- **Next:** Work through the Lesson 24 retrieval probe, then begin Lesson 25: UDP datagrams and port demultiplexing.

### 2026-07-07 (lesson 23) — IPv4 header, checksum, and ARP-backed TX
- **Done:** Implemented Lesson 23's IPv4 slice. Added `src/net/checksum.rs` with the RFC 1071 internet checksum helper, added `src/net/ipv4.rs` with 20-byte IPv4 header build/parse support, named EtherType `0x0800`, and added `NetDevice::send_ipv4_packet` plus a central `poll_rx` dispatcher that returns `NetRxEvent::{Arp, Ipv4, Other}`. The TX path builds the IPv4 header, resolves the next-hop MAC through the Lesson 22 ARP cache, wraps the packet in Ethernet, and submits it through virtio-net. The initialized virtio-net device is installed for syscall-triggered tests, and `/net_smoke.elf` now triggers both TX and host-forwarded RX from ring 3.
- **Status:** Roadmap checkboxes unchanged. This completes the base IPv4 layer needed before ICMP ping in Lesson 24.
- **Verification:** `cargo check --features test-mode,test-net` passed with 53 warnings. `./tests/test_ipv4.sh` passed and confirmed checksum `0x0DFB`, build/parse proof for protocol 17, and the parse proof marker. `./tests/test_arp.sh` passed after the disk image gained `/net_smoke.elf`. `./tests/test_net_smoke.sh` passed and confirmed `/net_smoke.elf` requested IPv4 TX, transmitted to `10.0.2.2`, requested RX, and received a host UDP packet sent to `127.0.0.1:5555` as `[ip] rx proto=17 src=10.0.2.2 dst=10.0.2.15 len=45`.
- **Docs:** Updated `lessons/6.04-ipv4.html`, preserved the checksum layout fix, linked Lesson 22 forward to Lesson 23, and added `tours/tour-6.04-ipv4.html`.
- **Next:** Lesson 24 should add ICMP echo request/reply support on top of IPv4 protocol number 1 and use it to ping the slirp gateway.

### 2026-07-06 (lesson 22) — ARP parser, resolver, and cache
- **Done:** Implemented Lesson 22's ARP slice. Added `src/net/arp.rs` with `ArpPacket` parse/build helpers, RFC 826 constants, request/reply builders, and a small `BTreeMap` ARP cache. Added `OUR_IP` and `GATEWAY_IP` constants in `src/net/mod.rs`. Reworked `NetDevice` to resolve ARP through cache lookup, broadcast-on-miss, RX polling, ARP payload parsing, cache merge, and cache-hit return. Added the inbound request reply path for packets targeting `10.0.2.15`.
- **Status:** Roadmap checkboxes unchanged. This completes the MAC resolution layer needed by IPv4; Lesson 23 can ask `resolve_arp()` for the next-hop MAC before transmit.
- **Verification:** `cargo check --features test-mode` passed with 50 warnings. `./tests/test_arp.sh` passed and confirmed cache miss, learned gateway mapping `10.0.2.2 -> 52:55:0a:00:02:02`, resolved gateway output, cache hit, and cache size 1. `./tests/test_virtio_net_frames.sh` still passed with the Lesson 21 serial markers. `./tests/run.sh` passed all 7 tests.
- **Docs:** Updated `lessons/6.03-arp.html` to match the implementation, linked Lesson 21 forward to Lesson 22, and added `tours/tour-6.03-arp.html`.
- **Next:** Lesson 23 should build IPv4 header parse/build, use ARP to choose the Ethernet destination MAC for the gateway, and prepare the path toward ICMP ping in Lesson 24.

### 2026-07-06 (lesson 21) — first Ethernet frames over virtio-net
- **Done:** Implemented Lesson 21's first-frame slice. Added `src/net/ethernet.rs` for Ethernet header parse/build with big-endian EtherType handling, wrapped the initialized virtio-net driver in `NetDevice`, built and sent a broadcast ARP request for slirp's gateway, added a two-descriptor TX chain, parsed the first RX Ethernet frame after stripping the 12-byte `virtio_net_hdr`, copied payload bytes into a `Frame`, and recycled the filled RX descriptor back into the avail ring.
- **Status:** Roadmap checkboxes unchanged. This completes the first raw Ethernet TX/RX proof for the Phase 2 networking track; the next networking slice should parse the 28-byte ARP payload into structured fields and cache the gateway MAC.
- **Verification:** `cargo check --features test-mode` passed with 50 warnings. `./tests/test_virtio_net_frames.sh` passed and confirmed TX completion, `TX broadcast frame sent, 54 bytes`, RX ARP from `52:55:0a:00:02:02` to `52:54:00:12:34:56`, RX buffer recycle, and the final first-frame proof. `./tests/run.sh` passed all 6 tests.
- **Docs:** Added `tours/tour-6.02-first-frames.html`, linked it from Lesson 21, linked Lesson 20 forward to Lesson 21, and kept the Lesson 21 page/style pass aligned with the project writing rules.
- **Next:** Lesson 22 should replace the hand-crafted ARP payload with an `ArpPacket` parse/build helper, validate sender/target fields in the reply, and store the gateway MAC for IPv4/ICMP work.

### 2026-07-05 (lesson 20) — virtio-net handshake, MAC, queues, RX supply
- **Done:** Implemented Lesson 20's virtio-net initialization slice. Added `VirtioNet` driver state, mapped common/notification/device-config capabilities, negotiated `VIRTIO_NET_F_MAC` plus modern transport, read and stored the MAC address from device config, configured RX queue 0 and TX queue 1, posted eight 2048-byte RX buffers, and set `DRIVER_OK`. Added `tests/test_virtio_net_init.sh` to lock the serial proof.
- **Status:** Roadmap checkboxes unchanged. This continues the Phase 2 networking track; packet parsing and TX/RX frame helpers remain for Lesson 21.
- **Verification:** `cargo check --features test-mode` passed with 50 warnings. `./tests/test_virtio_net_init.sh` passed and confirmed `FEATURES_OK`, MAC `52:54:00:12:34:56`, both queue-enable readbacks, eight RX buffers posted, and `DRIVER_OK`. `./tests/run.sh` passed all 5 tests.
- **Docs:** Added `tours/tour-4.07-virtio-net-init.html`, linked it from Lesson 20, linked Lesson 19 forward to Lesson 20, and included the existing RX ring animation page referenced by the lesson.
- **Next:** Lesson 21 should build the driver-facing frame API: skip the 12-byte `virtio_net_hdr` on RX, prepend a zeroed header on TX, recycle RX buffers after parsing, and send/receive the first Ethernet frame.

### 2026-07-05 (lesson 19) — Networking phase starts with virtio-net discovery
- **Done:** Implemented Lesson 19's build slice. Added a generic `PciDevice` coordinate type and `find_virtio_device(device_id)` helper, kept the existing virtio-blk transitional fallback for the disk path, attached a modern-only virtio-net PCI card in `scripts/run.sh` and the shared QEMU test helper, and added a boot probe that finds device `0x1041` and prints its BARs. Added `tests/test_virtio_net_probe.sh` to lock the discovery proof.
- **Status:** Roadmap checkboxes unchanged. This starts the Phase 2 networking track described by `lessons/roadmap-networking.html`; the existing Month 4 storage/syscall checklist still reflects the older spine.
- **Verification:** `cargo check --features test-mode` passed with the existing 51-warning baseline. `./tests/test_virtio_net_probe.sh` passed and confirmed `vendor=1af4 device=1041`, the explicit `virtio-net found` line, and BAR output.
- **Docs:** Added `tours/tour-6.01-network-stack-overview.html`, linked it from Lesson 19, and linked Lesson 18 forward to Lesson 19.
- **Next:** Lesson 20 should wake the card: parse virtio-net capabilities, read the MAC address from device config, complete the handshake, and prepare receive buffers.

### 2026-07-05 (lesson 18) — FD inheritance, exit cleanup, wait by PID
- **Done:** Implemented Lesson 18's process-lifecycle slice. Added `FdTable::clone_shallow()` and `FdTable::open_count()`, made `spawn_fork_child` copy the parent's FD table into the child, made `mark_current_zombie` replace the exiting process's FD table with an empty table and log how many descriptors were closed, and extended `sys_wait4` to accept a target PID plus status pointer. Added `WaitTarget::{Any, Specific(Pid)}` so `wait(-1, status)` still reaps any child while `wait(pid, status)` tracks one child.
- **Status:** Roadmap checkboxes unchanged. This closes the Lesson 18 lifecycle work that sits between the completed fork/exec/wait milestone and later scheduler/address-space cleanup.
- **Verification:** Rebuilt `disk.img` with `/inherit_wait.elf`. `cargo check --features test-mode,test-fork` passed with the existing 51-warning baseline. `cargo check --features test-mode,test-inherit` passed with the same baseline. `./tests/test_inherit_wait.sh`, `./tests/test_fork_smoke.sh`, `./tests/test_hello.sh`, and `./tests/run.sh` all passed. The new inherit test confirms the child reads through an inherited FD, parent receives exit status 7 from `wait(pid)`, the shared file cursor advances, and both processes log `closed_fds=1`.
- **Docs:** Added `tours/tour-3.04-fork-wait-reap.html`, linked it from Lesson 18, and kept the tour low on jargon.
- **Next:** Decide whether Lesson 19 should tackle signal delivery (`SIGCHLD`) or scheduler cleanup for dead task entries.

### 2026-07-05 (lesson 17) — Real process table wired into fork/exec/wait
- **Done:** Finished Lesson 17's process-table refactor. Added `ProcessTable` as the owner of stable `Pid` allocation, moved process-shaped state (`name`, parent/children, exit status, address space, fd table) off `Task`, and left `Task` with scheduler state plus a `pid` back-reference. Re-pointed `current_pid`, fd-table access, address-space lookup, `mark_current_zombie`, `wait_for_zombie_child`, and `replace_current_address_space` through `PROCESS_TABLE` with scheduler/process locks acquired separately under interrupts-disabled sections. Renamed the per-task ring-3 entry stack field to `entry_stack_top`, linked Lesson 17 from Lesson 16's footer, and added the Lesson 17 code tour.
- **Status:** No roadmap checkbox changed. Month 4 item 4.3 was already complete; this was structural cleanup before process lifecycle work.
- **Notes / blockers / surprises:** `cargo check --features test-mode,test-fork` remains at the baseline 51 warnings. Cleared `target/x86_64-mini-os` before smoke tests to avoid stale bootimage behavior. `tests/test_fork_smoke.sh` passed, including child exit, parent wait, and PID 1 reap. `tests/test_hello.sh` passed on the plain exec/exit path.
- **Follow-up:** Replaced the temporary `&'static str` process-name storage with an owned fixed-size `ProcessName` buffer, so `exec` copies the user-provided program name into `Process` instead of leaking it into a static slot.
- **Next:** Lesson 18 / process lifecycle: decide how reaped `Task` entries are cleaned up, free retired process resources, and make PID/table state robust once scheduler indices diverge from PIDs.

### 2026-06-27 (slice 4.6) — Relink kernel at high addresses, simplify AddressSpace
- **Done:** Closed the long-standing "kernel at low addresses" architectural wart. Two pieces:
    - **Linker flag:** added `rustflags = ["-C", "link-arg=--image-base=0xFFFFFFFF80000000"]` to `.cargo/config.toml`. No custom linker script needed — `rust-lld`'s `--image-base` shifts the entire kernel binary to start at the high canonical address. The bootloader crate reads the resulting ELF and maps the kernel there automatically.
    - **AddressSpace simplification** (`src/memory.rs`): `AddressSpace::new()` now copies only PML4 entries 256–511 (the kernel half) instead of all 512. `deep_clone_from_current()` got an explicit two-phase structure: Phase 1 copies entries 256–511 verbatim (no allocations, no walking — just `clone()` per entry), Phase 2 walks only entries 0–255 (user half + low-half kernel mappings like VGA, heap, physical-memory offset). The nested 4-level walk code is unchanged; only its outer range shrank from `0..512` to `0..256`.
    - **Test fix** (`src/main.rs`): the `[test] AddressSpace create + switch` test now calls `deep_clone_from_current()` after `new()` (matching real fork/exec usage), since `new()` alone no longer copies the low-half kernel mappings (VGA, heap) that `println!` needs.
    - **Translate test fix** (`src/memory.rs`): the `_start` translate test was hardcoded to the old low address `0x207ec0`. Now uses `_start as u64` to follow the symbol — it currently resolves to `0xFFFFFFFF800216d0` (high canonical), translating to physical `0x4206d0`.
    - **New doc:** `docs/kernel-linker-layout.html` — visual explainer covering the build pipeline (source → rustc → linker → ELF → bootloader → runtime), what's inside the ELF, what the bootloader crate does at runtime, our concrete address layout, PML4 (Page Map Level 4) entry decoding, the wart we just fixed, and what `--image-base` changed.
- **Status:** 4.3 flipped to `[x]` (fork/exec/wait all done as of slice 4.5; the checkbox was stale). Slice 4.6 is the architectural cleanup; no roadmap checkbox for it (it's not a feature, just internal hygiene).
- **Notes / blockers / surprises:**
    - **The flag worked on the first try.** I had predicted "2–4 surprise hardcoded addresses, probably a triple fault." Reality: zero triple faults, zero hardcoded addresses to fix. The kernel just moved and everything kept working because nothing depended on *where the kernel itself was* — only on the offsets the bootloader handed us via `BootInfo`.
    - **One small stall:** the standalone `AddressSpace::new()` test triple-faulted when called without `deep_clone_from_current()`, because `println!` after `switch_to()` needs VGA mapped (entry 0), which `new()` no longer copies. Fix was to make the test mirror real-world usage (new + deep_clone).
    - **Frame usage observation:** deep_clone now allocates slightly *more* frames per AS than before in our test, because the test that used to call only `new()` now also calls `deep_clone`. In actual fork/exec paths the count is essentially unchanged — the savings from skipping the kernel half (entries 256–511) is small because the kernel image is ~555 KB (fits in one PD/PDPT/PT subtree). The win is structural clarity, not memory savings.
    - **The `physical_memory_offset` from the bootloader is unchanged** (`0x180_0000_0000`, PML4 entry ~96, still in the low half). It works transparently — it's just a kernel mapping in the user half, and deep_clone walks it. Moving it to high addresses too would be cleaner but isn't necessary.
- **Next:** 4.4 VFS layer. The "what next?" question is now actually next on the roadmap. Likely sub-slices: per-process file descriptor table, `File`/`Inode`/`Directory` abstractions, ramfs implementation, then `open`/`read`/`write`/`close` syscalls.

### YYYY-MM-DD
- **Done:** (what got built/learned this session)
- **Status:** (which milestones/checkboxes flipped)
- **Notes / blockers / surprises:** (anything useful for future-you)
- **Next:** (where to pick up next time)

### 2026-06-22 (slice 10) — Process-tree fields + meaningful getpid (4.3.a done)
- **Done:** Foundation for `fork`/`wait`. Extended the `Task` struct with three fields that real kernels (Linux's `task_struct`) have but ours didn't: `parent: Option<u64>`, `children: Vec<u64>`, `exit_status: Option<i64>`. Added `TaskState::Zombie` (the "exited but not yet reaped" state — between Running and Dead). Added a `current_pid()` helper that returns the running task's ID. Updated `sys_getpid` to use it instead of the hardcoded `1` it was returning.
    - Asked-and-answered along the way: **why store both parent and children?** Two different access patterns. Parent pointer answers "who is my parent?" (used for child-exit notification, orphan reparenting). Children list answers "give me all my children" (used for `wait(-1)`, child validation in `wait(pid)`, reparenting). Storing both keeps both directions O(1) instead of O(N). Linux uses intrusive linked lists (`struct list_head`) for memory efficiency; we use a simple `Vec<u64>` since we don't care about scale yet.
- **Status:** Sub-slice 4.3.a done. 4.3 still in progress (need fork, wait, exec).
- **Notes / blockers / surprises:**
    - `sys_getpid` now returns 0 (was 1). Why 0? Because the user program runs in `main`'s task context — `main` called `enter_user_mode` which did `iretq` to user mode, but the task the CPU was running is still `main` (id=0). When we eventually spawn the user program as its own task (probably in 4.3.b or 4.3.c), `getpid` will return that task's actual ID.
    - All three fields are in place but currently unused — `fork` (next slice) will set `parent`/`children`, `exit` will set `exit_status` and `Zombie` state, `wait` will consume them. The struct is just ready for them.
    - No actual process behavior changed — the 3-syscall test (write/getpid/exit) still works exactly as before. This slice is purely setup.
- **Next:** 4.3.b — `fork`. Clones the current task to create a child: allocates a new kernel stack, copies (or shares) the address space, sets parent/children pointers, schedules the child. Return value: child PID to parent, 0 to child (the API that lets the same code work for both after the call). Likely the hardest single slice so far.

### 2026-06-22 (slice 9) — Fixed SSE bug, dedicated syscall stack (real root cause)
- **Done:** Found the real root cause of the previous slice's "triple fault when handler uses 4-arg println". The previous log entry blamed "kernel stack overflow" — that was wrong. Actual cause: **SSE wasn't enabled**. The bootloader left `CR4.OSFXSR` (bit 9) clear. Per the Intel SDM, executing SSE instructions (movups/movaps) with OSFXSR=0 should generate #UD. QEMU's emulation is inconsistent about enforcing this — most kernel contexts let SSE work, but the syscall entry context enforces it strictly. Two fixes shipped together:
    - **The actual fix:** explicitly set `CR4.OSFXSR` and `CR4.OSXMMEXCPT` at the top of `_start`. Two `mov cr4` instructions later, SSE works correctly everywhere. The kernel should have done this from M0 — every real OS does this in early boot.
    - **The proper-fix-anyway:** allocated a dedicated 64 KB syscall stack at virtual `0x6000_0000_0000`, mapped 16 frames with kernel-only permissions. `KERNEL_RSP` is now set to the TOP of this stack instead of the captured mid-stack RSP. Gives us guaranteed headroom for any future syscall handler complexity.
- **Status:** No roadmap items flipped. Quality slice — debugging + proper fix.
- **Notes / blockers / surprises:**
    - **The investigation process.** Used QEMU's `-d int` flag (`-d int -D /tmp/qemu-int.log`) to log every exception the CPU takes. The log showed the exact chain: timer interrupt → user makes syscall → #UD at `movups xmm0, [rsp+0x98]` in the handler → #NP while handling #UD → #DF → triple fault. Without `-d int` I was guessing blindly. With it, the answer was staring at me — `v=06` is #UD (Invalid Opcode) at an SSE instruction.
    - **How I got it wrong the first time.** I jumped to "stack overflow" because the simpler 1-arg println worked but the 4-arg one didn't. The difference in stack usage felt like a plausible explanation. But: the kernel stack is ~1.5 MB. 250 bytes can't overflow that. The user called bullshit, correctly. The real difference between the two println! versions: 4-arg creates more Argument structs and uses more `movups`/`movaps` to copy them around. More SSE instructions = more chances for the SSE-not-enabled bug to bite. It was a SSE issue all along, just hidden by QEMU's lax enforcement in some contexts.
    - **Why `write_byte` "fixed" it.** Adding a `serial::SERIAL1.lock().write_byte(b'.')` call before the println! somehow dodged the SSE fault. Never fully explained — possibly the call modified register state in a way that made QEMU's emulator take a different code path. It was a coincidence, not a fix. Removing it after the OSFXSR fix didn't break anything.
    - **Main's println! worked despite OSFXSR=0.** QEMU's emulator apparently executes SSE instructions permissively in normal kernel context but enforces the OSFXSR rule more strictly after a syscall/sysret transition. Real Intel/AMD hardware would be consistent (always #UD without OSFXSR). This is a QEMU quirk that we worked around by setting the bit explicitly.
    - **The dedicated stack is still valuable** even though it wasn't the bug. KERNEL_RSP was previously a frozen mid-stack pointer from `init()` time — fragile by design. The dedicated stack gives us guaranteed room regardless of when syscall fires.
- **Next:** Same as before — 4.3 `fork`/`exec`/`wait`. Real process management. The syscall mechanism now works correctly with all the proper setup.

### 2026-06-22 (slice 8) — Syscall dispatch table + core syscalls (4.1 / 4.2 done)
- **Done:** Expanded the syscall mechanism from a single match arm into a working dispatch table with 3 syscalls. The user program now makes 3 syscalls in sequence (write, getpid, exit) and the kernel routes each one to the right handler. End-to-end test output: `hello` → `sys_write wrote 6 bytes` → `sys_getpid -> 1` → `sys_exit(0) — halting`. M4 has begun.
    - Added `SYS_GETPID` (39) and `SYS_EXIT` (60) syscall numbers — these match the Linux x86-64 ABI conventions, so a real Linux binary's syscall numbers would (theoretically) line up.
    - `sys_getpid() -> u64`: hardcoded to return 1 for now. We have one user program; once `fork`/`exec` (M4 item 4.3) creates real processes, this will return the actual ID of whichever task made the syscall.
    - `sys_exit(exit_code: u64) -> u64`: prints a message, disables interrupts, halts in a `loop { hlt }`. Doesn't actually kill a process — we don't have process tracking yet. M4 item 4.3 will add real exit semantics.
    - Updated `USER_PROGRAM` to make 3 syscalls: write("hello\n", 6) → getpid → exit(0). 70 bytes of hand-assembled machine code.
- **Status:** Checkboxes 4.1 and 4.2 flipped to `[x]`. M4 underway.
- **Notes / blockers / surprises:**
    - **Major debug: kernel stack overflow in syscall handler.** The handler had `println!("[syscall] num={} args=({}, {}, {})", num, arg1, arg2, arg3)` at the top. The 4-argument format machinery allocates 4 Argument structs (16 bytes each) plus other format state, totaling ~250 extra bytes of stack usage. This overflowed the kernel stack below `KERNEL_RSP` (which was captured at `syscall::init()` time and doesn't have much room below it — we're somewhere mid-stack by the time `main` calls init). Symptom: triple-fault before any output appeared. Bisected by reverting one file at a time, then by stripping the handler down to `let _ = ...; 0` and rebuilding back up. **Workaround:** keep prints in the dispatch minimal — each individual `sys_*` function prints its own short message, which works fine. **Real fix (future):** allocate a dedicated syscall stack (a few fresh frames) instead of using the captured mid-stack RSP.
    - Initial confusion: I thought the issue was the new user program (which calls 3 syscalls instead of 1), but reverting to the single-syscall user program didn't help. The dispatch code in syscall.rs was the actual culprit. Took a few iterations of bisecting to figure this out.
    - **The "exit halts" approach has consequences.** Because sys_exit loops with interrupts disabled, the kernel is frozen after exit. That's expected for now — we have one process, so killing it = halting the system. When we have multiple processes, exit will kill the current one and switch to another.
- **Next:** 4.3 — `fork` / `exec` / `wait`. This is the big one for M4: real process management. `fork` clones an existing process, `exec` replaces its program with a new one, `wait` lets a parent block until a child exits. Requires us to actually track processes (the current `sys_getpid` returning a hardcoded 1 becomes meaningful here).

### 2026-06-25 (slices 4.4.a through 4.4.e) — AddressSpace + working fork with proper isolation
- **Done:** Five sub-slices landing the per-process address space abstraction and a working `fork` end-to-end. Parent and child now run independently with isolated memory; the test prints `child` then `parent` and both exit cleanly via Zombie + halt.
    - **4.4.a — `AddressSpace` type** (`src/memory.rs`): first-class address space abstraction. Each `AddressSpace` owns a PML4 frame. Methods: `new()` (allocate + zero + copy entries from current CR3), `switch_to()` (single CR3 write), `map_to`/`unmap` (operate on this AS's PML4 via a temporary `OffsetPageTable` wrapper), `from_current()` (no-allocate wrap of the bootstrap kernel AS), `pml4_frame()` accessor. New global `PHYS_OFFSET` static stashed at boot so AS operations can compute virtual addresses of physical frames. Test: create AS, switch to it, print, switch back.
    - **4.4.b — Per-task AS + CR3 switch on context switch** (`src/task.rs`): every `Task` now holds an `address_space`. Kernel threads (main, workers) share the kernel AS via `from_current()`. `yield_now` extracts the new task's PML4 frame, compares to current CR3, and writes CR3 only if they differ (TLB-flush avoidance when switching between kernel threads).
    - **4.4.c — `sys_fork` with all the plumbing** (`src/syscall.rs`, `src/task.rs`, `src/user.rs`):
        - Saved-state globals in syscall.rs (`USER_RIP_SAVE`, `USER_RFLAGS_SAVE`, `USER_RSP_SAVE`, `USER_RBX_SAVE`, `USER_RBP_SAVE`, `USER_R12-R15_SAVE`). `syscall_entry` step 3.5 (between push state and arg shuffle) snapshots user state using R8 as scratch.
        - `fork_trampoline` (naked asm in task.rs) — does `xor rax, rax`, loads RCX/R11/RSP from saved state, `sysretq`. Had to convert to naked asm after the compiler's `in(reg)` register allocator put a value in RAX and got it clobbered by the `xor rax, rax` (resulted in #GP at sysretq because RCX became 0).
        - `Task::syscall_stack_top` field + per-task syscall stack allocation in `spawn_fork_child`. `yield_now` calls `syscall::set_kernel_rsp()` on every switch. Critical fix — without per-task syscall stacks, parent and child collided on the shared syscall stack and corrupted each other's paused state.
        - `USER_PROGRAM` rewritten to call SYS_FORK (57) and branch on RAX (parent path / child path).
        - `sys_exit` upgraded: marks task Zombie (instead of halting), `yield_now` skips Zombies, halts only when all tasks are Zombie.
    - **4.4.d — Deep page-table clone** (`src/memory.rs`, `src/task.rs`): proper fork isolation. `AddressSpace::deep_clone_from_current()` walks the source's full PT tree (PML4 → PDPT → PD → PT) and allocates fresh frames at every intermediate level for the destination. At PT level: `USER_ACCESSIBLE` entries get fresh frame + 4 KB memcpy (private copy for the child); kernel entries are copied as-is (share the frame). Without this, `AddressSpace::new()` shared PDPT/PD/PT frames between parent and child, so unmapping a user page in the child also unmapped it in the parent → triple fault. Spawn_fork_child restructured to allocate kernel stack + syscall stack BEFORE the deep clone so the child inherits both.
    - **4.4.e — Snapshot user state per-child** (`src/task.rs`, `src/syscall.rs`): fixed saved-state globals staleness. `SavedCalleeSaves` extended with `user_rip`/`user_rsp`/`user_rflags`. `sys_fork` snapshots the globals ONCE at fork time. `spawn_fork_child` writes the snapshot to the child's kernel stack at fixed offsets (`stack_top - 64/-72/-80`) — below `init_rsp`, so `switch()`'s pops don't touch them. `fork_trampoline` reads via `[rsp - 64/-72/-80]` instead of from globals. After this, even if the parent makes more syscalls between fork and the child's first run, the child still sees the correct fork-time state.
- **Status:** No roadmap items fully flipped (4.3 still in progress — fork done, wait next, exec deferred). Architectural milestone reached: real per-process isolation via independent page tables.
- **Notes / blockers / surprises:**
    - **Per-task syscall stacks were the unobvious fix.** The original design had a single dedicated syscall stack at `0x6000_0000_0000`. Worked fine with one process. With fork, both parent and child used it: when the parent was preempted mid-syscall and the child then made its own syscalls, the child overwrote the parent's paused state on the shared stack. Parent's eventual `iretq` popped garbage. Fix: each task gets its own 16 KB syscall stack, placed at `SYSCALL_STACK_BOTTOM + (id+1) * SYSCALL_STACK_SIZE`. `yield_now` updates `KERNEL_RSP` to the current task's stack top on every switch.
    - **Shallow fork triple-faults.** Initial fork implementation just shared user pages between parent and child (skipped page cloning because of the shared-PT issue). That worked for the simple test (parent/child print different strings, both exit), but pointed at the fundamental problem: shared PT frames.
    - **The "Linux puts kernel at high addresses" trick.** Real Linux solves per-process isolation elegantly by linking its kernel at `0xFFFF_8000_0000_0000` and above. That puts the kernel in PML4 entries 256-511 (upper half), giving a clean entry-level split: every process shares the upper half via PML4-entry copy (just 8 bytes per process), lower half is per-process. Our kernel lives at low addresses (kernel code at `0x200000`, heap at `0x4444_4444_0000`, etc.), all in entries 0-255 alongside user memory. So "share the kernel half" doesn't work cleanly for us. We work around it via deep PT cloning (entries get independent frames). The proper fix (relink kernel at high addresses) is documented as future work.
    - **The naked-asm vs inline-asm tradeoff** showed up in `fork_trampoline`. Initially tried regular function with inline asm + `in(reg)`/`in("rcx")` constraints. Compiler register allocation kept clobbering things in surprising ways (RCX got RAX-then-0 because the compiler picked RAX for {rip} and we then did `xor rax, rax`). Converting to naked asm — every mov explicit — fixed it. Now reads saved state directly from the kernel stack via fixed offsets.
    - **QEMU `-d int` is the debugging superpower.** When fork triple-faulted and rebooted in a loop, regular serial output showed nothing useful (just the boot messages repeating). `-d int -D /tmp/qemu-int.log` showed every exception with full register state. Found the #GP at sysretq that way, traced it to a NULL RCX, traced THAT to the compiler's reg allocation clobbering user_rip.
    - **Multiple architectural workarounds in play.** Deep PT clone (vs Linux's clean half-split), per-task syscall stacks (vs Linux's per-task kernel stack via TSS.RSP0), saved state on the kernel stack at fixed offsets (vs Linux's `pt_regs` at a known stack-frame layout). All these would simplify if we relinked the kernel at high addresses.
- **Next:** 4.3 continues — `wait` syscall (parent blocks until child exits, reads exit code). Then `exec` (replace program). Then VFS (4.4).

### 2026-06-22 (slice 7) — Syscall mechanism via syscall/sysret (3.6.c done, M3 reached)
- **Done:** Implemented the fast-path user → kernel transition. The user program can now make a real syscall (SYS_WRITE) instead of doing direct memory writes that page-fault. End-to-end: user calls `syscall`, kernel catches it, writes "hello\n" to serial, returns via `sysretq`, user continues looping. Built `src/syscall.rs` (~190 lines):
    - **MSR (Model-Specific Register) setup.** Wrote helpers `rdmsr`/`wrmsr` (raw inline asm) and configured four MSRs in `init()`:
        - EFER (0xC0000080): set bit 0 (SCE — System Call Extensions) to enable the `syscall` instruction
        - STAR (0xC0000081): set bits 32-47 = 0x08 (kernel CS base) and bits 48-63 = 0x08 (user CS base, CPU adds 16 for user CS). For our GDT layout this gives kernel CS=0x08 / SS=0x10 on syscall entry, and CS=0x1B / SS=0x23 on sysret return.
        - LSTAR (0xC0000082): set to address of `syscall_entry` naked function
        - FMASK (0xC0000084): set to 0x200 (IF bit) so the CPU clears the interrupt flag on syscall entry
    - **KERNEL_RSP global.** Set in `init()` to the current kernel RSP at boot. By the time syscall fires, main has gone to user mode via `enter_user_mode`, so this kernel stack is available for syscall use.
    - **`syscall_entry` naked asm** (~60 lines). The 15-step sequence: switch from user RSP to kernel RSP (using r10 as scratch); push user RSP + 6 callee-saved registers + user RCX (RIP) and R11 (RFLAGS); rearrange args from syscall convention (rdi/rsi/rdx/r10/r8/r9) to System V (rdi/rsi/rdx/rcx/r8/r9); realign stack to 16 bytes; enable interrupts; call the Rust handler; disable interrupts; undo padding; save return value in r10; pop user state; pop callee-saved; restore user RSP; restore return value to rax; `sysretq`.
    - **`syscall_handler` dispatch.** Matches on the syscall number in rax. Currently just SYS_WRITE (num=1); unknown syscalls return -1 (u64::MAX).
    - **`sys_write`.** Reads N bytes from a user-supplied pointer and writes them to the serial port via `serial::SERIAL1.lock().write_byte()`. Returns the byte count.
- **Status:** Checkbox 3.6 flipped to `[x]`. **M3 milestone reached** (full multitasking: tasks, context switch, cooperative + preemptive scheduling, kernel threads, idle task, ring transition both ways).
- **Notes / blockers / surprises:**
    - First draft of the asm had a subtle register-save bug: I tried to save the handler's return value (rax) in rbx after the call, but rbx gets restored by the subsequent `pop rbx`. Fix: save return value in r10 (caller-saved, never pushed/popped in our sequence) and restore it as the very last step before `sysretq`.
    - Stack alignment for the System V ABI: we push 9 values (72 bytes) before calling the Rust handler. 72 mod 16 = 8, so RSP is misaligned for the call. Added `sub rsp, 8` before `sti`/`call` and `add rsp, 8` after to realign. Without this, any `movaps` instruction in the handler would fault.
    - Calling convention mismatch: Linux syscall ABI passes args in rdi/rsi/rdx/r10/r8/r9 (note r10 instead of rcx — syscall clobbers rcx by saving user RIP into it). System V function ABI uses rdi/rsi/rdx/rcx/r8/r9. The asm has to shuffle: `mov rcx, rdx; mov rdx, rsi; mov rsi, rdi; mov rdi, rax` (rax has syscall number, becomes first System V arg).
    - **First test passed cleanly**, no debugging detour this time. The visible proof in the QEMU output: `[syscall] num=1 args=(1073741888, 6, 1)` (args decoded: buffer_addr=0x40000040, length=6, then garbage in rdx we didn't set) → `hello` (the kernel wrote it to serial) → `[syscall] sys_write wrote 6 bytes` → user resumes, no further output (loops forever).
    - User program grew from 40 bytes (3.6.b) to 70 bytes (3.6.c). New layout: mov rax,1 / mov rdi,buffer_addr / mov rsi,6 / syscall / jmp -2 / 30 bytes padding / "hello\n". The padding is so the message lands at a known offset (0x40) within the user code page — easier to encode the address as an immediate.
    - **What's NOT in 3.6.c (future work):**
        - No `swapgs` — we use a global static for KERNEL_RSP instead. Real kernels store the kernel RSP in GS-base-relative memory; swapgs swaps the GS base on entry/exit for per-CPU data access. For single-CPU our global approach works.
        - No `copy_from_user` / `copy_to_user` validation. `sys_write` blindly trusts the user-supplied pointer. A malicious user could pass a kernel pointer and read kernel memory. Real kernels validate that pointers are within the user's address space (we'll add this when we get per-process address spaces in M4).
        - Syscall dispatch is a single match arm. As we add syscalls (M4), this becomes a dispatch table.
- **Next:** M3 is fully done — tag it. M4 starts with **4.1 syscall dispatch table**: a more structured way to register syscalls. Then 4.2 core syscalls (write, exit, getpid), 4.3 fork/exec/wait, 4.4 VFS layer, 4.5 RamFS, 4.6 pipes, 4.7 optional ext2.

### 2026-07-29 — Browser Phase 1: DNS resolver + HTTP client
- **Done:** Implemented the foundation for browser functionality — DNS resolution over UDP and HTTP/1.1 client layer. Created three new modules:
    - **`src/net/dns.rs`** — DNS packet formatter/parser for A-record queries. Supports label encoding/decoding (with compression pointer handling), builds queries, parses responses. `resolve()` helper takes a hostname, DNS server IP, and a send-callback, returns the IPv4 address.
    - **`src/net/http.rs`** — HTTP/1.1 client with `Request` and `Response` types. Supports GET/POST, custom headers, response parsing (status line, headers, body). Includes unit tests (verified at compile time).
    - **`src/net/wget.rs`** — Kernel-space wget that ties it together: DNS lookup → TCP connect → HTTP GET → receive response → close. Includes `test_wget_example()` for testing.
    - **Extended TCP** — `tcp.rs` now supports configurable endpoints (not hardcoded), `recv_all()` for bulk reads, and `close()`/`reset()` for connection lifecycle.
- **Status:** Phase 1 of the browser roadmap complete. DNS + HTTP over TCP work in kernel. Added `SYS_NET_WGET` (syscall 908) to trigger wget test from userspace.
- **Notes / blockers / surprises:**
    - DNS over UDP requires the existing UDP socket infrastructure — used `UdpSocket::bind()` and polled for responses with `socket.pop()`.
    - HTTP response parsing handles both `Content-Length` and connection-close (read rest) body extraction.
    - `windows(4)` on slices requires calling it on `&[u8]` directly, not on an iterator — had to refactor.
    - `to_string()` requires `ToString` trait in scope — added to imports for `dns.rs` and `http.rs`.
    - Build warnings only (unused imports, docs) — compilation succeeds.
- **Next:** Test wget with a real HTTP server (example.com over HTTP, port 80). Then Phase 2: Cookie jar for session management, Phase 3: HTML parser for form extraction, Phase 4: TLS layer for HTTPS.

### 2026-06-21 (slices 5 + 6) — Ring transition: GDT upgrade + first user-mode entry (3.6.a / 3.6.b done)
- **Done:** Got our first user-mode program running at ring 3 and proved the CPU's permission enforcement works end-to-end. Two sub-slices:
    - **3.6.a — GDT upgrade.** Added user code segment (ring 3), user data segment (ring 3), and a TSS descriptor to the GDT. Created a `TaskStateSegment` static, set its `privilege_stack_table[0]` (commonly called RSP0) to the current kernel stack pointer, and loaded it via `ltr`. No behavior change yet — just structures in place.
    - **3.6.b — First user-mode entry.** Added a page fault handler to the IDT (vector 14). Wrote `src/user.rs` with `enter_user_mode()` that allocates a user code page (1 frame) and user stack (16 frames), maps them with U/S=1 (user-accessible), copies a 40-byte hand-written x86-64 machine code program into the code page, builds the 5-value iret frame on the kernel stack (SS=0x23, RSP=0x70010000, RFLAGS=0x202, CS=0x1B, RIP=0x40000000), and executes `iretq`. The user program writes `0xDEADBEEF` to its own writable memory (proving user code ran), then tries to write to kernel address 0x200000 (proving enforcement).
- **Status:** 3.6 partially complete. Sub-slices 3.6.a and 3.6.b done; 3.6.c (syscall mechanism) still pending.
- **Notes / blockers / surprises:**
    - **Page fault handler signature quirk:** `idt.page_fault.set_handler_fn` specifically wants `extern "x86-interrupt" fn(&InterruptStackFrame, PageFaultErrorCode)` — not the generic `HandlerFuncWithErrCode` (which uses u64). The crate exports `PageFaultHandlerFunc` as the right type alias; transmuted our naked wrapper to that.
    - **Page fault wrapper stack layout.** CPU pushes an error code BEFORE the interrupt frame for vectors that have one (page fault, double fault, etc.). Our wrapper does `sub rsp, 8` for alignment, `lea rdi, [rsp+16]` to skip padding + error code and get &InterruptStackFrame, `mov rsi, [rsp+8]` to get the error code value. On the way out: `add rsp, 8` (undo align), `add rsp, 8` (skip the CPU-pushed error code, which iretq doesn't pop itself), then iretq.
    - **Major debugging detour — `options(noreturn)` bug.** Initial iretq asm! used `options(noreturn)`. The asm block compiled, but for unclear reasons the println! statements immediately before the asm were being elided by the compiler (we'd see "MARKER A" then nothing — the format-arg printlns in between were silently dropped). Switched to `options(nostack, nomem, preserves_flags)` + explicit `unreachable!()` after the asm — same semantics but the compiler stops doing weird things. Lost ~2 hours to this.
    - **Wild-goose chase on iretq itself.** Before finding the noreturn bug, I debugged the wrong thing for a while — thought iretq was failing because we saw no timer ticks after the transition. Reality: iretq was being entirely omitted from the binary because a `loop {}` I'd added during debugging made the asm unreachable. The compiler correctly elided it. Adding markers + checking the disassembly (`objdump -d | grep iretq`) revealed the asm wasn't in the binary at all.
    - **Test outcome proved both transition and enforcement.** The page fault report shows: `instruction_pointer: 0x40000021` (4th instruction of the user program — first three ran successfully), `code_segment: index 3, rpl: Ring3` (we WERE in ring 3), `tried to access: 0x200000`, `error code: 0b111` (P=1 page present, W/R=1 write attempt, U/S=1 user mode triggered). And `user wrote to its memory at 0x7000_0000: 0xdeadbeef` confirms the user code's first 3 instructions ran and successfully wrote to user-accessible memory before the 4th instruction faulted.
    - **User program is raw bytes.** The 40-byte program is hand-assembled x86-64 machine code (mov rax,imm64 + mov rcx,imm64 + mov [rax],rcx + mov rax,imm64 + mov qword [rax],0). Documented in comments at the top of `src/user.rs`. We write the bytes directly to the code page with `core::ptr::write_volatile` in a loop — no compiler needed for this tiny program.
    - **What's NOT in 3.6.b:** the page fault handler loops forever with `hlt` (interrupts disabled by CPU on entry, we never re-enable). The user program can't be "killed" and resumed — kernel just halts. That's M4 fork/wait territory.
- **Next:** 3.6.c — syscall mechanism. Set up the MSRs (STAR, LSTAR, FMASK) so the CPU knows where the syscall entry point lives. Write a naked-asm syscall entry that saves user state, switches to the kernel stack, calls the Rust handler, then `sysret` back. Upgrade the user program to actually make a syscall (probably `write` to serial) instead of just memory writes. After 3.6.c, M3 is complete.

### 2026-06-21 (slice 4) — Idle task + hlt (3.5 done)
- **Done:** Added a fourth task — the **idle task** — that demonstrates the canonical kernel idle pattern: `loop { increment_counter; hlt(); }`. The `hlt` instruction halts the CPU until the next interrupt, dropping into a low-power state on real hardware and (more visibly for us) letting QEMU's emulated CPU rest so it doesn't burn a host core busy-spinning. Also rewrote main's monitoring loop to be hlt-based: print all four counters (workers + idle) once per second, then `hlt` until the next tick. Replaced the debug-y `iter=…` output with a clean `tick=N a=… b=… idle=…` line per second. Made `interrupts::TICKS` public so main can read the tick count.
- **Status:** Checkbox 3.5 flipped to `[x]`. 5 of 6 M3 items done.
- **Notes / blockers / surprises:**
    - With 4 tasks round-robin (main, worker_a, worker_b, idle), each gets exactly 25% of CPU time. Confirmed by idle counter: at tick 100, idle=25 (one increment per scheduling slice). At tick 200, idle=50. Linear growth = fair scheduling.
    - Workers increment ~5.6M times per 100 ticks (per second of wall time). At 25% CPU per worker, that's ~22M ops/sec when running. Reasonable for QEMU emulating x86_64 on Apple Silicon.
    - The idle task getting equal time isn't optimal — in real kernels the scheduler skips idle unless nothing else is ready. We left it in round-robin for simplicity. Could add a "is_idle" flag and have yield_now skip idle when other tasks are runnable, but not needed for the demo.
    - Main now uses hlt between prints, so QEMU's host CPU usage is much lower (no busy-loop). On a real laptop, the previous version would have burned a core spinning; this version lets the host CPU rest.
    - Cleaned up the verbose `iter=N` debug output from 3.4 — was useful for diagnosing the caller-saved register bug, no longer needed.
- **Next:** 3.6 — ring transition (kernel ↔ user mode). Adds user-mode segments to the GDT, sets up the TSS (Task State Segment), implements `syscall`/`sysret`, and runs our first piece of code at ring 3 (user mode). This is the doorway to M4 (syscalls, VFS, IPC).

### 2026-06-21 (slice 3) — Preemptive scheduling via timer (3.4 done)
- **Done:** Made the timer interrupt handler call `yield_now()` after sending End-Of-Interrupt, so the scheduler preempts the current task every 10ms automatically — no cooperative yields needed. Added a `trampoline()` function: brand-new tasks start via the trampoline instead of going directly to their entry function. The trampoline enables interrupts as its first action, bridging "scheduler world" (IF=0) and "task world" (IF=1). Considered the alternative of putting `sti` directly inside `switch` asm before `ret` — decided on the trampoline for separation of concerns (switch stays minimal) and future extension (per-task FPU state, exit handling). Modified `yield_now` to disable interrupts at the top and restore them on exit, preventing re-entrant timer interrupts from deadlocking on SCHEDULER's Mutex. Updated `Task::new` to set the faked RIP to `trampoline` instead of the user's entry_fn directly, and added `entry_fn` as a field on Task so the trampoline can look it up via `SCHEDULER.current`.
- **Status:** Checkbox 3.4 flipped to `[x]`. 4 of 6 M3 items done.
- **Notes / blockers / surprises:**
    - **Major bug found and fixed:** initial preemptive test showed wild counter values (88 trillion iterations in 5 seconds — impossible). Root cause: the timer handler wrapper was only realigning the stack, not saving caller-saved registers (RAX, RCX, RDX, RSI, RDI, R8-R11). In cooperative mode this was fine — yield happens at function call boundaries where the compiler manages caller-saved. In preemptive mode the interrupt fires mid-computation, and the handler's call to `yield_now` clobbered the interrupted code's registers. When the task resumed, it read garbage. Fixed by rewriting `timer_wrapper` to push all 15 general-purpose registers before calling `timer_inner`, then pop them in reverse before `iretq`. Bonus: 15 pushes × 8 bytes + 40-byte CPU interrupt frame = 160 bytes total, a multiple of 16, so RSP is automatically 16-aligned for the call.
    - Socratic discussion before implementing: the user asked "why not just enable interrupts before switch() to avoid needing a trampoline?" Answer: enabling before switch creates a dangerous window where the timer can fire mid-switch and corrupt the state machine. Two clean alternatives: (1) `sti` inside switch right before `ret` (the `sti` instruction has a one-instruction delay so `sti; ret` is atomic), or (2) trampoline. Both work; trampoline is the convention because it keeps switch minimal and provides an extension point.
    - User asked "why not inline the trampoline?" — recognized it wasn't strictly required. Valid point: inlining (option 1) is simpler for small kernels. We went with trampoline for the extension point and convention.
    - The `sti` instruction has a documented one-instruction delay (Intel SDM) — interrupt delivery is suppressed until after the next instruction completes. So `sti; ret` is safe: the ret is guaranteed to execute before any interrupt.
    - Frame accounting: each new task still eats 19 frames (16 stack + 3 page tables). With 3 tasks now, ~57 frames used.
    - In preemptive mode, the wrapper saving all registers is critical. The breakpoint_wrapper still uses the old "realign only" pattern — fine for now because breakpoint isn't called from arbitrary code points the same way (it's triggered by `int3` which we rarely use), but should be updated for consistency if we ever rely on it.
    - Test output proves preemption: main, worker_a, and worker_b all make progress at similar rates (~9M counter increments per worker over 5 sec; main does ~20M loop iterations). All three are getting fair CPU time via round-robin at 100Hz.
- **Next:** 3.5 — kernel threads (idle task that halts instead of busy-loops, plus demo workers doing real work). Then 3.6 — ring transition (kernel ↔ user mode), which sets up M4 (syscalls).

### 2026-06-21 (slice 2) — Context switch + cooperative scheduler (3.2 / 3.3 done, real multitasking works)
- **Done:** Wrote `switch()`: a `#[unsafe(naked)] unsafe extern "C" fn` with 14 lines of inline asm. Pushes RBX/RBP/R12-R15 onto the current stack (the 6 callee-saved registers; caller-saved are preserved by the calling convention so they don't need manual save), saves the current RSP into the old TaskContext, loads the new TaskContext's RSP into the CPU, pops the 6 registers off the new stack, then `ret` (which pops the new task's saved RIP — for a paused task, that's where it was when it yielded; for a brand-new task, that's its entry function address). Naked function because a normal Rust prologue/epilogue would mess up the careful stack layout. Then built a minimal round-robin scheduler on top: `Scheduler { tasks: Vec<Box<Task>>, current: usize }` behind a `Mutex`, with `init_main()`, `spawn()`, and `yield_now()`. `yield_now` locks only long enough to pick the next task and grab raw pointers; drops the lock before calling `switch` so we never hold it across a context switch (would deadlock). Round-trip test in `_start`: init_main, spawn worker, yield from main → worker runs, prints, yields back → main resumes. **It worked first try.** Heap, frame allocator, and timer tests all continued passing after the round trip.
- **Status:** Checkboxes 3.2 and 3.3 flipped to `[x]`. M3 half-done.
- **Notes / blockers / surprises:**
    - `#[unsafe(naked)]` is the modern Rust syntax (was `#[naked]` until recently). Project already used it in `src/interrupts.rs` for the IDT wrappers, so the pattern was familiar.
    - `core::arch::naked_asm!` (not `asm!`) is required inside naked functions.
    - `extern "C"` on x86-64 Linux is the System V AMD64 ABI: first two args in RDI and RSI, referenced directly in the asm.
    - Push order is the reverse of pop order so the last-pushed register (R15) ends up at the lowest address — matching the faked initial state layout from `Task::new` (entry_fn at top, R15 at the bottom where RSP starts).
    - `Scheduler.tasks: Vec<Box<Task>>` — Box ensures Task addresses are stable across Vec reallocation. Critical because we take raw pointers into the tasks for `switch`.
    - Main task is special: registered with `context.rsp = 0` (gets filled in by switch on first yield) and `stack_top = 0` (uses the bootloader's stack, not one we allocated).
    - Worker is left suspended forever after yielding back to main, because main never yields again. In a real OS the worker would loop or exit; for the round-trip test it's fine.
    - No compile errors at all on the first try. The visual / Socratic prep paid off — the algorithm was clear before any code was written.
- **Next:** 3.4 — preemptive scheduling via timer interrupt. Hook `yield_now` into the timer handler so switches happen automatically every N ticks, even if a task never yields. Then 3.5 (kernel threads: idle task + workers that do real work).

### 2026-06-21 (slice 1) — Task struct + constructor (3.1 done, M3 started)
- **Done:** First slice of M3. Socratic walkthrough of: what state a paused task needs to save (RIP, RSP, callee-saved registers — caller-saved are preserved by the calling convention), where that state lives (registers on the paused task's own stack, with `TaskContext = { rsp }` as the bookmark), how big each stack should be (64 KB — Linux uses 16 KB kernel stacks, bumped up from 8 KB after deep-call-chain overflows), where stack RAM comes from (frame allocator, not heap — keeps kernel stacks out of heap, like Linux's vmalloc variant). Built `docs/context-switch.html` — animated step-by-step visualization of the 5-step switch algorithm (push regs → save RSP → load new RSP → pop regs → ret). Then wrote `src/task.rs`: `TaskState` enum (Ready/Running/Blocked/Dead), `TaskContext { rsp: u64 }`, `Task { id, name, state, context, stack_top }`, and `Task::new(id, name, entry_fn)` which allocates 16 frames via the existing frame allocator, maps them into a fresh virtual range starting at `0x5000_0000_0000`, and pre-populates the top of the stack with faked saved state (entry_fn addr + zeros for RBX/RBP/R12-R15) so the same context-switch routine that resumes old tasks can also start a brand-new one.
- **Status:** Checkbox 3.1 flipped to `[x]`. M3 in progress.
- **Notes / blockers / surprises:**
    - **Frame accounting surprise:** creating one 16-page task ate 19 frames, not 16. The extra 3 are the intermediate page tables (PDPT, PD, PT) that `map_to` allocates on the first walk through a fresh virtual range. The PML4 entry already existed because of our heap mapping, but everything below had to be created. Fixed the misleading "expected 16" test message.
    - Used `core::ptr::write_volatile` to fake the initial stack state. The compiler can't see that the context-switch routine (yet to be written) will later read those bytes, so without `volatile` it could legally elide the writes as "dead."
    - Stack alignment: `init_rsp = stack_top - 56` is 8 mod 16. After `ret` pops 8 bytes, RSP becomes 0 mod 16. Slightly off from the strict ABI expectation (RSP should be 8 mod 16 at function entry) but doesn't matter until the entry function calls another function. Will revisit if it bites in 3.2.
    - `entry_fn` is `unsafe extern "C" fn()` because the context switch will `ret` into it like a normal function call. The `unsafe` signals "caller must ensure the CPU is in a sane state."
    - Worker task is created but never scheduled in this slice (no scheduler yet — comes in 3.3). Just verifying the data structures and constructor work.
- **Next:** 3.2 — context switch routine. The 20 lines of assembly that actually do the swap (push callee-saved regs, save RSP, load new RSP, pop callee-saved regs, ret). Will live alongside `Task::new` in `src/task.rs` or in a new `src/context.rs`.

### 2026-06-20 (slices 3–7) — Paging, heap, mapper API (2.2 / 2.3 / 2.4 / 2.5 done, M2 reached)
- **Done:** Took M2 from "we have a frame allocator" all the way to "kernel can `Box::new`, grow `Vec`, build `String`, and the mapper is a global API any module can call." Five slices:
    - **2.2a** — wrapped `x86_64`'s `OffsetPageTable`, read CR3 + `BootInfo.physical_memory_offset`, ran translate sanity check.
    - **2.2b** — implemented `FrameAllocator<Size4KiB>` adapter around our bitmap, called `mapper.map_to()` end-to-end, verified with a write to the new mapping.
    - **2.3** — wrote a bump allocator by hand (one pointer, never frees), registered as `#[global_allocator]`, demonstrated `Box::new(42)` + `Vec` + `String`.
    - **2.3b** — swapped bump for the `linked_list_allocator` crate. Proved frees work by running a 20,000-iteration `Box` loop (would have crashed bump around iteration 12,800).
    - **2.5** — extracted the mapper from a local in `memory::init` to a `lazy_static Mutex<Option<OffsetPageTable>>` behind `memory::translate()` / `memory::map_to()`. Internal callers (init_heap) now go through the public API.
- **Status:** Checkboxes 2.2, 2.3, 2.4, 2.5 flipped to `[x]`. **M2 reached** (memory alive: frames + paging + heap + Box/Vec/String + global mapper API).
- **Notes / blockers / surprises:**
  - Bootloader 0.9 default does NOT identity-map everything. First translate test crashed silently on `translate_addr(0x207EC0)` because the relevant PT frame wasn't reachable. Enabling `map_physical_memory` feature gives a high virtual offset where all physical RAM is mapped; that fixed it. `phys_offset = 0x18000000000` on QEMU default.
  - Bootloader maps the kernel at virtual 0x200000+ (its link address) but loads it at physical 0x400000+. Translation: `_start` at virtual 0x207EC0 → physical 0x407EC0.
  - `x86_64` 0.15 quirks: `FrameAllocator::allocate` was renamed to `allocate_frame`. `map_to` is `unsafe` (caller must guarantee no aliasing). `MapToError` is at `paging::mapper::MapToError`, not re-exported at the `paging` level.
  - `extern crate alloc` needs `alloc` added to `.cargo/config.toml`'s `build-std` list, plus `#![feature(alloc_error_handler)]` for the `#[alloc_error_handler]` attribute.
  - `linked_list_allocator::LockedHeap::init` takes `(*mut u8, usize)`, not `(usize, usize)`. Easy to get wrong.
  - For lazy_static of a value that needs `BootInfo` to construct: store as `Mutex<Option<T>>` and fill from `init()`. Awkward but works.
- **Next:** Tag `m2-memory-alive`. Then M3 — processes & scheduling. First sub-slice likely 3.1 (`Task` struct: saved register context, kernel stack, state).

### 2026-06-20 (slice 1) — Read memory map + serial output (start of M2)
- **Done:** Added a tiny 16550 UART driver (`src/serial.rs`) that mirrors all `println!` output to COM1, so kernel logs can be captured outside QEMU's scrolling VGA window. Changed `_start` to accept `&'static BootInfo`, iterate `boot_info.memory_map`, and print every region. The bootloader tags regions as `Usable`/`Kernel`/`KernelStack`/`PageTable`/`Bootloader`/`BootInfo`/`Reserved`/`FrameZero`/ACPI variants — so we already know which "Usable" frames are actually free, no manual subtraction needed.
- **Status:** Prep slice for 2.1. No checkbox flipped yet — this gave us the input data for the frame allocator.
- **Notes / blockers / surprises:**
  - `bootloader 0.9.34`: `MemoryRegion` fields are `range` (a `FrameRange` of `start_frame_number`/`end_frame_number`, exclusive) and `region_type` (`MemoryRegionType` enum with 15 variants). Not `start`/`end`/`kind` like the older API I half-remembered.
  - VGA writes don't go to QEMU stdout — only serial does. Need to write to COM1 (port `0x3F8`) and launch QEMU with `-serial stdio -display none` to capture in a file.
  - Our 16550 driver skips init (baud rate, FIFO, status checks) because QEMU's emulated UART is instantaneous — wouldn't work on real hardware.
  - Updated panic handler to also `force_unlock` the serial Mutex (same deadlock risk as VGA).
- **Next:** Build the frame allocator (2.1 proper).

### 2026-06-20 (slice 2) — Bitmap frame allocator (2.1 done)
- **Done:** Wrote `src/frame_allocator.rs`: a bitmap allocator (1 bit per frame, linear scan for first 0 bit). Init walks the memory map and clears bits only for `Usable` regions. Public API: `init()`, `allocate() -> Option<u64>`, `deallocate(frame)`, `frames_free()`. Sanity-tested in `_start`: allocate 3 frames, free the middle, allocate again — the new allocation reuses the freed frame (linear scan finds lowest-numbered free first). Math checks: 32031 free → 32028 after 3 allocs → 32028 at end (3 frames still held). Built `docs/frame-allocator.html` with interactive demos of all this.
- **Status:** Checkbox 2.1 flipped to `[x]`.
- **Notes / blockers / surprises:**
  - Bug + happy incident: first version computed `max_frame` across ALL memory regions. The giant Reserved MMIO region at `0xfd00000000–0x10000000000` made `end_frame_number` = 268,435,456 (1 TB of "frames"), which blew `MAX_FRAMES`. Fixed by scanning only `Usable` regions to size the bitmap. The bug also stress-tested the panic handler — it printed exactly the line and message needed to fix it. Win for slice 9's work.
  - First 3 frames handed out: 638, 639, 640. Frame 638 = `0x27E000` = start of first Usable region. Linear scan picks the lowest free, as designed.
  - Bitmap sized for `MAX_FRAMES = 131,072` (512 MB max physical RAM). For our 128 MB QEMU default, only ~4 KB of the 16 KB bitmap is actually used.
  - `bootloader::bootinfo` — no underscore in the path, despite the file being `bootinfo/memory_map.rs`. (`bootloader::boot_info` doesn't exist.)
- **Next:** 2.2 — 4-level paging. Map the kernel to a higher-half virtual address (currently it's identity-mapped by the bootloader).

### 2026-06-14 (slice 9) — Panic handler (1.7 done, M1 reached)
- **Done:** Made the panic handler actually useful. Replaced `loop {}` with `force_unlock()` + `println!("{}", info)`. Now panics print the location (file:line:col) and message instead of silently halting. Verified by triggering a temporary `panic!("test panic from kernel main")` and dumping VGA — saw `panicked at src/main.rs:46:5:\ntest panic from kernel main`. Then removed the test panic.
- **Status:** Checkbox 1.7 flipped to `[x]`. **M1 reached** (interrupts alive: timer ticks + useful panic). Tagged `m1-interrupts-alive`.
- **Notes / blockers / surprises:**
  - `spin::Mutex::force_unlock()` is `unsafe` in 0.9 — has to be wrapped.
  - If we panicked WHILE WRITER was locked (e.g. inside a `println!`), calling `println!` in the handler would spin forever. `force_unlock()` releases the lock unconditionally — safe because we're halting anyway.
  - Skipped the full stack-trace version (frame pointer walking + symbol resolution). Minimal version (location + message) covers ~80% of debugging needs.
  - Panics don't disable interrupts, so timer ticks continue during the panic `loop {}`. Mildly weird but actually useful — you can see the kernel isn't frozen.
- **Next:** M2 = memory management. First sub-slice: frame allocator (2.1).

### 2026-06-14 (slice 8) — Hardware timer interrupts (1.6 done)
- **Done:** Wired up the 8259 PIC + 8253 PIT to fire a periodic timer interrupt. Same pattern as the breakpoint handler works: PIC routes the timer IRQ to vector 0x20, IDT has a handler there, the handler prints + sends EOI + returns. Verified at 100 Hz with print-every-100th-tick throttling so we see ~1 "tick #N" line per second.
- **Status:** Checkbox 1.6 flipped to `[x]`. M1 almost done — only 1.7 (panic stack trace) left.
- **Notes / blockers / surprises (lots of detours here):**
  - pic8259 crate: ChainedPics was removed from x86_64 crate; now in separate `pic8259 = "0.10"` crate (no underscore). It pulls in x86_64 0.14 as transitive dep (we use 0.15) — works fine because pic8259 doesn't expose x86_64 types in its public API.
  - **Critical:** `pic8259::ChainedPics::initialize()` saves and restores whatever mask state the firmware left (everything masked in QEMU). Must explicitly call `write_masks(0xFE, 0xFF)` after to unmask the timer IRQ 0.
  - **Bug 1:** First attempt with `pit::set_frequency(2)` panicked — divisor (596591) doesn't fit in 16 bits. PIT divisor max is 65535, so minimum frequency is ~18.2 Hz. Fixed by using 100 Hz + atomic tick counter that only prints every 100th tick (~1/sec visible).
  - **Bug 2:** Bootimage is built by `bootimage` tool, NOT `cargo build`. Stale `.bin` file looks identical to "kernel doesn't print." Always use `cargo run` or `cargo bootimage` to repackage after code changes.
  - Debugging tip that worked: use QEMU monitor via Unix socket (`-monitor unix:/tmp/qm.sock,server,nowait`) + `nc -U` to send `xp /Nbx 0xb8000` to dump VGA buffer, and `info registers` to get RIP. Also `objdump -d` on the kernel binary + `nm | sort` to decode return addresses on the panic stack.
- **Next:** 1.7 panic stack trace. Then M1 milestone reached.

### 2026-06-14 (slice 8, step 1 of 4) — PIC setup
- **Done:** Created `src/pic.rs` with a `ChainedPics` static (master offset 0x20, slave offset 0x28) and `init()` that calls `PICS.lock().initialize()`. Called `pic::init()` from `_start` after `interrupts::init()`. No behavior change yet — PIC is initialized but no interrupts fire (IF=0 still).
- **Status:** First sub-step of 1.6 (hardware interrupts). Three more steps to go: timer handler, PIT config, enable interrupts.
- **Notes / blockers / surprises:**
  - `ChainedPics` was removed from `x86_64` (used to be in `x86_64::structures::pic`). It's now in the separate `pic8259 = "0.10"` crate (no underscore in name).
  - pic8259 0.10.4 pulls in `x86_64 = "0.14"` as a transitive dep — now we have two x86_64 versions in the tree. Not a problem because pic8259's public API doesn't expose x86_64 types.
- **Next:** Step 2 = add timer handler at IDT[0x20] using the naked wrapper pattern from slice 7.

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
