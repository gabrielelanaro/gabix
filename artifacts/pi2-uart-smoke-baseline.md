# Raspberry Pi 2 UART smoke baseline

This file records the physical control image that boots successfully on the
project Raspberry Pi 2B. Keep this image unchanged. Derive every experimental
image into a new file with a new registry id and checksum.

## Preserved artifact

- Registry id: `pi2-uart-smoke-working-2026-08-09`
- Durable path:
  `~/Documents/Gabix-Images/gabix-pi2-uart-smoke-working-2026-08-09.img`
- Size: `2,499,805,184` bytes
- SHA-256:
  `66abb107a93d2db17729e7fcc5bedc2c4c6be15c5d2844d0e29d0306e870d5c6`
- Protection: read-only permissions plus macOS user-immutable flag (`uchg`)
- Layout: full official image with a 536.9 MB FAT32 `bootfs` partition and a
  2.0 GB Linux partition

Verify it before use:

```sh
python3 scripts/pi2-image-registry.py verify \
  pi2-uart-smoke-working-2026-08-09
```

## Physical verification on 2026-08-15

The image was written verbatim to the confirmed external removable 7.9 GB SD
device at `/dev/rdisk7` in 149 blocks of 16 MiB. `dd` wrote all
`2,499,805,184` bytes and returned `FLASH_STATUS:0`.

Mounted-card comparison passed for all firmware-facing files:

```text
BOOTCODE.BIN=PASS
START.ELF=PASS
FIXUP.DAT=PASS
KERNEL7.IMG=PASS
CONFIG.TXT=PASS
```

After insertion into the physical Pi and a power cycle, the sole UART monitor
at 115200 baud captured:

```text
gabix uart smoke booted
gabix uart smoke alive
```

The capture contained more than 10,000 complete heartbeat lines during this
test. This independently reproduced the successful physical boot from
2026-08-09.

## Known property of this exact image

`fsck_msdos -n` reports that `kernel7.img` owns more clusters than its 260-byte
logical size requires. The builder replaced the original Linux kernel contents
and retained its contiguous cluster chain. Raspberry Pi firmware boots this
exact layout successfully. Preserve this property in the control image; test
filesystem cleanup only in a separately named candidate.

## Failed image that must not be used as the baseline

The 545,259,520-byte intermediate image with SHA-256
`d844e1dc13e2f25acb60754f993d4bfd4ac83f19db42e90204c697091a26bbc2`
produced four ACT flashes and no UART output. Its registry status is
`hardware-failed`.
