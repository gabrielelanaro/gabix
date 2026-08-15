# Use whole-kernel restart for hot reload

Status: accepted

Gabix v1 defines hot reload as a whole-kernel restart through a UART loader. The
loader is reloaded from the unchanged SD card after reset, receives the new kernel
into RAM, verifies it, and transfers control to it. This gives each iteration a
clean CPU, interrupt, cache, and device state; live patching of an already-running
kernel remains a later experiment. A healthy kernel requests the reset through the
`reboot` system call; a physical reset remains the recovery path for a wedged kernel.

## Considered options

- Whole-kernel restart through a UART loader — selected for clean state and a small
  recovery surface.
- Live patching of individual kernel functions — deferred because it requires state
  migration and safe coordination with active interrupts and devices.

The reboot system call is the kernel interface for the reset. UART input may later
drive a user program or shell that invokes the call, but a UART command is not part
of the kernel reboot contract.
