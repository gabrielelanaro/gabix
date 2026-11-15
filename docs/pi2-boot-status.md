# SD boot implementation status

Updated November 15, 2026.

The connected Pi now boots the SD loader and saved kernel automatically.
Installation and all resets used the existing Ethernet/UART connections; the
card stayed in the Pi.

| Piece | State |
|---|---|
| SD reads/writes and corrected clock | Tested on the physical Pi |
| Ethernet staging and direct inactive-kernel uploads | Physical transfer and complete byte-for-byte read-back passed |
| Two 32 MiB kernel areas | Installed in a new 128 MiB partition; original partition locations preserved |
| Boot-record validation and SD kernel loading | Host tests and physical boots passed |
| Hung candidate → previous working kernel | Watchdog reset and automatic fallback passed with the installed loader |
| Both copies invalid → UART recovery | Rejected both checksums, accepted a UART kernel, restored the record and booted from SD |
| Permanent loader installation | 42 changed FAT data/directory sectors read back exactly; subsequent firmware boot passed |
| Recovery image | Built from the protected original; loader, record and kernel contents verified; equivalent configuration tested on hardware |

The final card contains two valid 83,864-byte development kernels. Copy A is
confirmed active, with no pending or failed attempts. The service accepts a new
kernel over Ethernet and preserves the currently confirmed copy.

The recovery image is registered as `pi2-sd-boot-2026-11-15` in
`artifacts/pi2-images.json`. The original protected UART-loader image remains
unchanged. The new image's complete raw-file SHA256 is recorded there; this run
installed its changes in place rather than flashing the complete image.

The new partition starts at sector 4,882,432. Kernel A starts at 4,884,480;
B starts at 4,950,016. Each has 65,536 sectors (32 MiB). Metadata reserves
1 MiB; 63 MiB remains spare. The executable-size limit is currently 16 MiB.

See [SD boot and update instructions](pi2-sd-boot.md) for the boot record,
commands, tests, and single-record power-loss limitation. Raw logs, packages,
and intermediate images remain in ignored `target/runs/` output.
