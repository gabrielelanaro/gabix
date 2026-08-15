# Gabix boot-development context

This context defines the vocabulary for rapidly loading and restarting Gabix on
the Raspberry Pi 2 during kernel development.

## Language

**Hot reload**:
Replacing the entire running kernel by resetting through the UART loader and
uploading a new kernel into RAM. _Avoid_: live patching, in-place function patching

**UART loader**:
A small boot program loaded from the SD card by the Raspberry Pi firmware. It
receives and verifies a development kernel over UART, then transfers control to it.
_Avoid_: UART shell, live updater

**Loader reboot**:
A normal SoC reset requested by the Gabix `reboot` system call so the firmware
reloads the UART loader. A physical reset is the fallback when the kernel cannot
run the system call. _Avoid_: UART reboot command, SD-card reflashing

**Reboot system call**:
The user-facing kernel interface that requests a board reset. Its implementation
selects the platform reset mechanism; UART is only an optional transport for a
program or shell that invokes it.

**Single-user reboot permission**:
In v1, every Gabix user program may invoke the reboot system call because the
system runs one user program and has no credential or capability model.

**Non-returning reboot**:
After a successful reboot request, control never returns to the caller because the
board reset restarts the machine. An unexpected return is a reset failure and must
halt safely.

**Platform reset adapter**:
The architecture-specific implementation behind the no-argument reboot system call.
The Raspberry Pi adapter writes the board reset registers; user programs do not know
those addresses.

**Development command chain**:
The explicit sequence of existing host commands used to build, start the UART
monitor, upload a kernel, and inspect logs. The workflow is composed from separate
commands rather than hidden behind one launcher.

**Kernel upload**:
The host-to-Pi transfer of a development kernel after the UART loader starts. The
loader accepts the complete byte stream only when its declared size and CRC32 match.
_Avoid_: SD-card update

**Loader address**:
The fixed RAM address, `0x00008000`, where Raspberry Pi firmware loads and starts
the UART loader.

**Development load address**:
The fixed RAM address, `0x00100000`, where the UART loader places and starts the
uploaded Gabix kernel. The kernel is linked for this address.

**Development-loader mode**:
The UART loader startup mode used during kernel development: after reset it waits
for a host upload indefinitely and does not silently boot an older kernel from SD.
_Avoid_: fallback boot

**Upload marker**:
The fixed `GABIXLOAD` byte sequence that begins a kernel upload. The loader scans
for this marker before reading the upload header.

**Failed upload**:
An upload whose size, timeout, or CRC32 check fails. The loader never starts it;
it resets its receive state, reports the failure, and waits for another upload.

**Valid upload**:
An upload whose declared size is allowed and whose received CRC32 matches. The
loader reports success and immediately starts the development kernel.

**Upload size limit**:
The v1 loader accepts development kernels up to 16 MiB starting at the development
load address. Larger declarations are rejected before the loader receives payload
bytes.

**Upload timeout**:
The v1 loader waits indefinitely for the upload marker, then allows at most five
seconds without receiving a payload byte before declaring the upload failed.
