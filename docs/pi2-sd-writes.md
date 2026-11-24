# Pi 2B sector writes

`WritableBlockDevice` extends the shared read interface with one 512-byte sector
write and an explicit error result. The Pi implementation is in
`targets/armv7/src/raspi2b/sdhci.rs`. Existing read-only implementations keep their
current interface.

The driver sends CMD24, waits for the controller's buffer-write-ready flag,
feeds 128 words into its data FIFO, and waits for transfer completion. It then
uses CMD13 to check the card's error bits, ready-for-data flag, and transfer
state. Each controller wait has a one-second system-timer deadline. An error
leaves the write outcome uncertain; reinitialize the card before further I/O.
Power-loss recovery belongs in the future update protocol.

The driver queries the firmware's EMMC source clock and calculates dividers for
at most 400 kHz during initialization and 25 MHz for default-speed transfers.
The divider is rounded up to respect each limit. On the connected Pi, firmware
reported 200 MHz. The old hard-coded divider of 1 therefore requested 100 MHz,
although the driver had never negotiated a faster card mode.

Read-only experiments held the transfer code and clock-switch sequence fixed:
- Old divider 1: command timeout/CRC errors.
- Temporary divider 128 (781.25 kHz): 128 identical sector reads in 811,179 µs.
- Divider 4 (25 MHz): the same 128 reads in 139,932 µs, about 5.8 times faster.

These are software elapsed times for repeated single-sector PIO reads, not a
bulk-storage benchmark or a measurement of the clock pin. The firmware rate,
divider calculation, error flags, and successful 25 MHz run identify the old
clock configuration as the problem. The final firmware-derived divider passed
the physical write/read/restore test. Four-bit transfers, DMA, multi-block I/O,
and higher-speed negotiation remain possible future performance work.

## Physical test

The board must be at the UART loader. A running development kernel can return
there using the existing UART userspace reboot program. The test uploads a RAM
kernel and resets automatically; its watchdog also returns a hung test to the
loader. A loader return alone is insufficient: the runner requires the PASS line.

```sh
sh tests/test_sd_write_restores_sector.sh /dev/cu.usbserial-0001
```

For the installed three-partition development card, select the managed layout:

```sh
sh tests/test_sd_write_restores_sector.sh /dev/cu.usbserial-0001 managed-layout-scratch
```

This mode uses sector 1024 within the kernel partition's reserved metadata
area (absolute sector 4,883,456). It checks the exact installed partition table,
a valid boot record with no pending update, kernel copies starting at or after
sector 2048, card capacity, and zero contents in the scratch sector and both
neighbours. The boot record occupies relative sector 0; persistent logs begin
at relative sector 133120. This opt-in avoids staging-header sector 8192,
which belongs to the Ethernet transfer service on an installed card.

On 2026-11-16 the managed-layout test passed on the physical Pi: pattern
read-back, restoration, both neighbour comparisons and automatic reset all
succeeded. A read-only host check saved the boot record and three empty sectors
before the test; no kernel image, boot record or staging header was modified.

The test deliberately targets the registered `pi2-uart-loader-full-2026-08-15`
recovery image in `artifacts/pi2-images.json`. Its saved original has an entirely
zero-filled gap between the MBR and the first partition at sector 16384. Sector
8192 is reserved here as test scratch space. Before writing, the fixture checks
the disk signature, all four partition entries, the MBR signature, and zero
contents in the scratch sector and its neighbors. These checks apply to this
known image; they do not establish safety for arbitrary cards with similar layouts.

The test writes a deterministic pattern, reads back all 512 bytes, restores the
original sector, verifies the restoration, and compares both neighbors. Raw
serial output stays under ignored `target/runs/`. Ordinary builds of
`pi2-sd-check` are read-only; the runner supplies the explicit write opt-in.

On November 15, 2026 the connected Pi passed this sequence and returned to
`GABIX LOADER READY` without a power cycle. This verifies the sector write path;
Ethernet upload, boot slots, and power-loss recovery are separate work.

References:
- [SD Physical Layer specification](https://www.sdcard.org/cms/wp-content/themes/sdcard-org/dl.php?f=Part1_Physical_Layer_Simplified_Specification_Ver7.10.pdf): default-speed limit, CMD24/CMD13, R1 status.
- [BCM2835 peripherals](https://pip-assets.raspberrypi.com/categories/579-raspberry-pi-zero/documents/RP-008249-DS-1-bcm2835-peripherals.pdf): section 5, CMDTM, DATA, INTERRUPT, CONTROL1.
- [Firmware mailbox interface](https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface): Get clock rate, EMMC clock ID.

