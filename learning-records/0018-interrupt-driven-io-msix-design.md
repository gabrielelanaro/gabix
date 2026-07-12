# Interrupt-driven I/O phase chosen — MSI-X over legacy INTx

The user chose to replace all polling with hardware interrupts before
pursuing TCP or page cache. The kernel currently has one interrupt
source (PIT timer via 8259 PIC, vector 0x20); all virtio I/O is
busy-wait polling (200k iteration loop in `read_sector`, cooperative
spin-yield `network_worker`). The design decision is MSI-X + Local
APIC rather than legacy INTx through the existing PIC, matching what
Linux's `virtio_pci_common.c` does in production. Two lessons: L28
gets virtio-blk interrupt-driven end-to-end (APIC enable, MSI-X
table parse/program, IDT ISR, `hlt`-based wait replacing the poll
loop); L29 generalizes to virtio-net (delete `network_worker`,
ISR-driven RX/TX, blocking `recvfrom`/`poll` wakeups, lost-wakeup
race against hardware interrupts).

**Why MSI-X, not INTx:** The 8259 PIC's shared IRQ lines require
the ISR status register (virtio cap cfg_type 3) for disambiguation
and level-triggered handling. MSI-X gives each virtqueue a dedicated
vector via a DMA write to the APIC at `0xFEE00000` — edge-triggered,
no ISR read, no sharing. The ISR capability stays unmapped.

**Implications:** Enabling the Local APIC does not break the PIC
(QEMU virtual-wire compatibility mode). The timer continues through
the PIC; device interrupts arrive via APIC EOI (`0xFEE000B0`), not
PIC EOI. The naked-asm ISR wrapper follows the existing
`timer_wrapper` shape (push 15 registers, call inner, pop, iretq).
Adding Phase 2b to MISSION.md between Phase 2 and Phase 3.
