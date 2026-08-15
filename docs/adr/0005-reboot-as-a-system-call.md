# Expose reboot as a system call

Status: accepted

Gabix will expose board reset through a `reboot` system call. The syscall enters
the kernel through the normal ARM exception path and delegates to the Raspberry Pi
reset implementation. User programs and a future shell can invoke it; UART remains
an optional input transport and does not define the kernel reset interface. This
matches the operating-system model and keeps the reset mechanism independent of the
development loader.

Every user program may invoke the syscall in v1 because Gabix is single-user and
does not yet implement credentials or capabilities. A future multi-process security
model can restrict it.

The syscall takes no arguments and does not return after a successful reset. The
platform reset implementation owns the Raspberry Pi register sequence; an
unexpected return is treated as failure and halts the machine.

The syscall interface is platform-independent and delegates to a platform reset
adapter so user programs do not depend on Raspberry Pi MMIO addresses.
