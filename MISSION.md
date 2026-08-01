# Mission: Understand how a computer works by running gabix on a Raspberry Pi

## Why
Hobby project: learn how a computer works by building and running a Linux-like
kernel on two CPUs — x86_64 and ARMv7 — and on real hardware. The gabix kernel
already boots on QEMU x86_64, on QEMU ARM (`virt` and `raspi2b`), and reaches
userspace on all three. The focus right now is ARM and the Raspberry Pi 2B
because the hardware arrives Monday. The broader goal is permanent: understand
both architectures, not just ARM.

A concrete north star keeps the work pointed: **post a message to LinkedIn from
a gabix user program running on the Pi.** The post is an excuse; the
understanding earned along the way is the goal. Expect side explorations (a
keyboard, a display) whenever they teach something worth knowing.

The previous mission (x86 disk → ext2 → ELF → ring 3) is complete — see
learning-record 0017 and git history.

## Success looks like
- Boot gabix on a physical Raspberry Pi 2B and see the full serial sequence in
  `docs/raspberry-pi-2.md` through `userspace exited`.
- Explain, from the gabix source, how the same kernel runs on x86_64 and ARMv7:
  privilege and exception entry, address translation, device discovery, and the
  syscall mechanism on each.
- The Pi gets a DHCP lease, answers ping, and echoes UDP on port 5555.
- Bring up a peripheral the kernel does not drive yet (e.g. a USB keyboard) and
  read input from it in a user program.
- Post a message to LinkedIn from a gabix user program on the Pi (north star).

## Constraints
- Rust, `#![no_std]`, bare metal, no external runtime deps.
- Hardware: Raspberry Pi 2B (ARMv7-A, Broadcom BCM2836). QEMU `raspi2b` and
  `virt` machines emulate before metal.
- macOS build host (qemu, e2fsprogs, coreutils via Homebrew).
- Both x86_64 and ARMv7 are first-class. A new concept is taught on both where
  it applies, not on one with the other as an afterthought.
- Cite primary sources (ARMv7-AR Reference Manual, BCM2836 ARM Peripherals
  manual, Intel SDM for x86, QEMU `virt` doc). Follow the "Linux to read
  alongside" pattern.
- Plainspoken prose in all lessons and records. Avoid the words "shape",
  "boundary", and "whole" — the user finds them vague or filler. Name the
  component, the action, and the value.

## Modules (parallel — grab any thread)

The course is seven topic modules plus an orientation. Lessons are numbered
`module.lesson` (e.g. `4.07`) so modules can advance in parallel. The full
per-lesson index lives in `reference/modules.html`.

| Module | What it covers | Status |
|---|---|---|
| 0 — Orientation | The course map, cross-cutting overviews, roadmaps, spaced-repetition reviews. Read `0.01` first. | 0.01 written |
| 1 — The CPU | How the CPU lets a kernel protect itself and transfer control: privilege (x86 rings vs ARM modes), exception and interrupt entry (IDT vs vector table + interrupt controller), the syscall mechanism (`syscall`/`sysret` vs `svc`). | new — both archs |
| 2 — Memory | Address translation and per-process address spaces: x86 four-level paging + CR3 vs ARM short-descriptor sections + TTBR0; the MMU enable sequence; caches. | new — both archs |
| 3 — Processes | Running programs: context switching (x86 vs ARM), ELF loading (ELF64 vs ELF32), `fork`/`exec`/`wait`. | 4 lessons (x86); ARM context switch to add |
| 4 — Devices | How the kernel finds and talks to hardware: PCI vs memory-mapped discovery, MMIO, DMA, virtio (over PCI on x86, over MMIO on ARM), interrupt-driven I/O (MSI-X + Local APIC vs GIC). | 11 lessons (x86); ARM virtio-MMIO + keyboard to add |
| 5 — Storage | The block layer, the virtual file system, and ext2 on disk. | 7 lessons (complete, arch-neutral) |
| 6 — Networking | Ethernet → ARP → IPv4 → ICMP → UDP → sockets → DHCP → TCP. The codecs are arch-neutral; the x86 lessons transfer to ARM. | 13 lessons (x86); reuses on ARM, HTTP/TLS to add |
| 7 — Real hardware | The Raspberry Pi 2B's Broadcom BCM2836 SoC: SD card via SDHCI, the DWC2 USB host, the LAN9512 USB-Ethernet chip, the mailbox property interface, the BCM local interrupt controller and timer. What a real chip does that the architecture manual does not describe. | new — what you debug Monday |

Modules 1, 2, and 7 are where the new ARM learning lives. Modules 3–6 already
have working x86 lessons; the ARM port reuses 5 and 6 nearly verbatim and
re-does 3 and 4 for the ARM CPU and its memory-mapped devices.

## Out of scope (for now)
- **ARMv8 / AArch64.** The Pi 2B is ARMv7-A. AArch64 is a later target if you
  move to a Pi 3 or 4.
- **A full POSIX userland or shell.** gabix runs one program at a time.
- **Hand-rolling TLS to LinkedIn's bar.** Modern TLS plus certificate chains to
  a real service is a large effort. Realistic path: a fixed-config minimal TLS
  client, or tunneling. The goal is understanding the network path, not writing
  crypto.
