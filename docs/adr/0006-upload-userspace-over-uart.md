# Upload userspace programs to the resident development kernel

Status: accepted

The Raspberry Pi development kernel receives its first userspace ELF over
UART. It validates the existing framed-upload header and CRC, loads the ELF
through the normal ARM user loader, and enters it. The kernel returns to the
UART receiver after the program invokes `exit`, allowing repeated userspace
uploads without resetting the board or writing the SD card.

The exit path resets the SVC stack before entering the receiver again. Each
uploaded program therefore starts with the same kernel-stack capacity even
though control does not unwind through the previous Rust call frames.

The SD card remains the recovery root. Raspberry Pi firmware loads the fixed
UART kernel loader from it after power-on or watchdog reset. Kernel changes use
the existing whole-kernel UART upload. User-program changes use a separate host
command and keep the current kernel resident in RAM.

The first interface accepts one ELF of at most 512 KiB. A later initramfs can
extend the payload to multiple named files when Gabix needs a userspace tree.
The single-ELF interface keeps framing, validation, memory ownership, and retry
behavior concentrated in one module while that additional filesystem behavior
is absent.
