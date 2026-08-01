# Gabix

Gabix is a small Linux-like operating system written in Rust. The repository
contains the x86-64 kernel, shared kernel services, ARMv7 adapters, QEMU
behavioral tests, and the Raspberry Pi 2B SD-card image builder.

## Build on macOS

Install the host tools:

```sh
brew install qemu coreutils e2fsprogs python
```

Install Rust with `rustup`, then enter the repository. The
`rust-toolchain.toml` file selects the pinned nightly compiler and requests the
`rust-src` and `llvm-tools-preview` components:

```sh
rustup target add x86_64-unknown-none --toolchain nightly-2026-04-30
```

Build the Raspberry Pi 2B image:

```sh
sh scripts/make-pi2-sd-image.sh
python3 scripts/verify-pi2-image.py target/gabix-pi2.img
```

The first command builds the ARM kernel, compiles the ARM `/init.elf` fixture,
downloads the pinned Raspberry Pi firmware files, creates the FAT16 boot and
ext2 root partitions, and writes `target/gabix-pi2.img`. The second command
checks the MBR, boot files, and ext2 signature.

The builder needs `mkfs.ext2` and `debugfs`. Homebrew installs them under the
`e2fsprogs` prefix, which the scripts search automatically. Set
`PI_FIRMWARE_DIR` to a directory containing `bootcode.bin`, `start.elf`, and
`fixup.dat` to reuse a local firmware copy.

## Run the behavioral tests

QEMU provides the x86-64 and ARM emulators. Homebrew installs
`qemu-system-arm` on the `PATH`; set `QEMU_SYSTEM_ARM` explicitly if another
ARM QEMU binary should run the tests:

```sh
QEMU_SYSTEM_ARM="$(command -v qemu-system-arm)" \
QEMU_TIMEOUT=30 \
sh tests/run.sh
```

The tests build their own kernels and disk fixtures. They assert observable
behavior from outside the kernel, including x86 process and filesystem calls,
ARM boot and system calls, Raspberry Pi SD and ext2 reads, USB enumeration,
DHCP, and UDP echo.

## Put the image on the Pi

Use Raspberry Pi Imager to write `target/gabix-pi2.img` to an SD card. Then
connect the Pi 2B Ethernet port to the router and follow
[`docs/raspberry-pi-2.md`](docs/raspberry-pi-2.md) for the DHCP, ping, UDP, and
optional serial checks.

The physical Pi verification remains separate from QEMU. QEMU verifies the
ARM and emulated USB/RNDIS paths; the board verifies the DWC2 timing, LAN9512
PHY, EEPROM MAC read, and real Ethernet link.
