# SD boot implementation status

Updated November 15, 2026.

| Piece | State |
|---|---|
| SD sector reads/writes and corrected clock | Tested on the physical Pi; committed and pushed |
| Ethernet file storage and verification after reboot | Tested on the physical Pi; committed and pushed |
| Interactive SD lesson | Browser-tested; committed and pushed |
| Card capacity | Read on the physical Pi: 15,523,840 sectors, 7,948,206,080 bytes; final sector readable |
| Two 32 MiB kernel areas | Assigned in storage-layout.json and checked against the existing MBR; not installed |
| Boot-record decoder and selected-kernel loader | Implemented; build and two host tests pass; not tested on the Pi |
| New → previous → UART automatic fallback | Not implemented |
| Updated burnable recovery image | Not built |
| New loader permanently installed on SD | Not installed |

The SD card still boots the existing UART loader. Its partition table and boot
files have not been changed. Only the previously tested Ethernet staging area
contains a stored test file. The most recent capacity diagnostic returned the
Pi to that existing UART loader.

The next delivery milestone is permanent SD boot with the agreed fallback.
Finish fallback, test the candidate loader through a RAM-only trial, generate
and verify a burnable recovery image, then install the tested layout and loader
on the connected card. Keep the old protected recovery image.

The planned new 128 MiB partition begins at sector 4,882,432, immediately after
the existing partitions. Kernel A starts at 4,884,480; B at 4,950,016. Each has
65,536 sectors (32 MiB). Metadata occupies the first 1 MiB; 63 MiB remains spare.
The current loader executable-size limit is still 16 MiB. The layout allocates
future capacity; increasing that limit requires a separate RAM-layout change.
