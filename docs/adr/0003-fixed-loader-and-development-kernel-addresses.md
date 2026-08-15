# Keep separate loader and development-kernel addresses

Status: accepted

The Raspberry Pi firmware loads the UART loader at `0x00008000`, matching the
existing `kernel7.img` boot contract. The loader places the uploaded development
kernel at `0x00100000`, and that kernel is linked for the same address. The
separation keeps the loader available while it receives the kernel and gives the
loader an unambiguous entry point for the raw binary.

The loader caps the development kernel at 16 MiB, ending at `0x01100000`, so a
corrupted size field cannot make an upload overwrite other reserved memory.

The repository will expose separate ARM build targets for the loader at `0x00008000`
and the development kernel at `0x00100000`. The host upload step sends the raw
development-kernel image.
