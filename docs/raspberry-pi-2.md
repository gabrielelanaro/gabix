# Raspberry Pi 2B runbook

Gabix boots from a partitioned SD-card image, reads `/init.elf` from the ext2
root partition, obtains an IPv4 address from the router by DHCP, replies to ICMP
echo requests, and echoes UDP datagrams sent to port 5555.

## Build and verify the image

Run these commands from the repository root:

```sh
sh scripts/make-pi2-sd-image.sh
python3 scripts/verify-pi2-image.py target/gabix-pi2.img
```

The build script creates `target/gabix-pi2.img`. The verifier checks the MBR,
the FAT16 boot partition, its five required boot files, and the ext2 root
partition.

The image contains:

| Partition | Contents |
|---|---|
| FAT16 boot | `bootcode.bin`, `start.elf`, `fixup.dat`, `kernel7.img`, `config.txt` |
| ext2 root | `/init.elf` and the filesystem test data |

## Isolate UART from Gabix

Build the boot-only UART smoke image when the first question is whether the Pi
can load a kernel and transmit one line:

```sh
sh scripts/make-pi2-uart-smoke-image.sh target/pi2-uart-smoke.img
python3 scripts/verify-pi2-uart-smoke-image.py target/pi2-uart-smoke.img
```

The image contains the pinned Pi firmware, `kernel7.img`, and `config.txt`.
The kernel configures GPIO14/15 and the PL011 directly, prints
`gabix uart smoke booted`, then repeats `gabix uart smoke alive`. It does not
initialize an SD driver, filesystem, USB stack, network, allocator, or Gabix
userspace. QEMU tests the same kernel logic with:

```sh
GABIX_PI2_UART_SMOKE_KERNEL_BASE=0x10000 \
  sh scripts/build-pi2-uart-smoke.sh target/pi2-uart-smoke/kernel-qemu.img
```

QEMU's `-kernel` loader uses `0x10000`; the physical image uses the Pi
firmware's `0x8000` load address from `config.txt`.

The build downloads `bootcode.bin`, `start.elf`, and `fixup.dat` from the pinned
Raspberry Pi firmware revision on its first run. Set `PI_FIRMWARE_DIR` to a
directory containing those three files to build without a network request.

## Write the image to an SD card

1. Open Raspberry Pi Imager.
2. Select **Choose OS**, then **Use custom**.
3. Select `target/gabix-pi2.img`.
4. Select the SD card and write the image.
5. Eject the card after verification completes.

Raspberry Pi Imager shows the destination card before writing and reduces the
risk of selecting a system disk with a raw-device command.

## Connect the Pi

1. Insert the SD card into a Raspberry Pi 2B.
2. Connect the Pi's Ethernet port to a LAN port on the router with a normal
   Ethernet cable.
3. Optionally connect a 3.3 V USB-to-TTL serial adapter as described below.
4. Apply power to the Pi.

The LAN9512 driver reads the board's Ethernet MAC address from EEPROM. If that
read fails, Gabix uses the locally administered fallback address
`02:47:41:42:49:58`.

Open the router's DHCP client or lease page and find the new lease by its MAC
address. Gabix does not currently send a DHCP hostname. Write down the assigned
IPv4 address.

## Test the network

Replace `PI_ADDRESS` with the address shown by the router:

```sh
ping PI_ADDRESS
printf 'hello from mac\n' | nc -u -w 2 PI_ADDRESS 5555
```

`ping` should receive ICMP echo replies. The `nc` command should print
`hello from mac` when it receives the UDP echo reply. Some `nc` versions exit
before printing a UDP response; this Python command performs the same check:

```sh
python3 -c 'import socket; s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM); s.settimeout(2); s.sendto(b"hello from mac",("PI_ADDRESS",5555)); print(s.recvfrom(1500)[0].decode())'
```

## UART baseline and serial diagnostics

Use a 3.3 V USB-to-TTL serial adapter. Connect these pins while the Pi is
powered off:

| Raspberry Pi header | Serial adapter |
|---|---|
| Pin 6, ground | Ground |
| Pin 8, GPIO14 / Pi TX | Adapter RX |
| Pin 10, GPIO15 / Pi RX | Adapter TX, optional |

Do not connect the adapter's 5 V or 3.3 V power pin. The repository provides one
UART monitor command so that only one process can own the serial device and each
capture has an explicit log path. Close any manually opened `screen`, `cu`, or
serial-monitor window first, then run:

```sh
sh scripts/monitor-pi2-uart.sh status /dev/cu.usbserial-DEVICE
sh scripts/monitor-pi2-uart.sh start /dev/cu.usbserial-DEVICE
```

The start command prints the session, device, baud rate, and log path. Follow
the printed `screenlog.0` while power-cycling the Pi. Stop the monitor after the
capture:

```sh
sh scripts/monitor-pi2-uart.sh stop
```

The monitor uses 115200 baud, 8 data bits, no parity, and 1 stop bit. The
`status` command reports the screen session and every process holding the
device. A second monitor is rejected before it can steal or split the UART
stream.

Use this order for every physical experiment:

1. Run `python3 scripts/verify-pi2-image.py target/gabix-pi2.img` and record the
   image checksum with `shasum -a 256 target/gabix-pi2.img`.
2. Start exactly one UART monitor.
3. Power-cycle the Pi and keep the complete log.
4. Classify the boot from the log: old NOOBS/Raspbian text proves the UART and
   old card work; `gabix raspi2b booted` proves that the Gabix image booted.
5. Stop the monitor before changing the card or starting another experiment.

This separates the UART baseline, the old-card baseline, and the Gabix boot
baseline. A silent log with `owners=none` means the monitor was not attached.
A silent log with one owner means the adapter is open but the Pi has not sent
data; check Pi power, GPIO14/TX to adapter RX, and shared ground.

The successful boot sequence prints these lines in order:

```text
gabix raspi2b booted
gabix raspi2b memory mapped
gabix raspi2b heap allocated
gabix raspi2b file read
gabix raspi2b usb enumerated
gabix raspi2b usb network configured
gabix raspi2b dhcp configured
gabix raspi2b timer advanced
gabix arm userspace wrote
gabix raspi2b userspace exited
```

## Preserve and classify physical SD images

`artifacts/pi2-images.json` is the tracked source of truth for every physical
Pi image. Each entry records the exact byte size, SHA-256, test status, source,
and physical evidence. Use a new image id whenever the bytes change. The three
statuses have fixed meanings:

- `candidate`: repository checks passed; no physical conclusion exists.
- `hardware-passed`: the physical Pi produced the recorded expected output.
- `hardware-failed`: the physical Pi produced the recorded failure.

List the registry before choosing an image:

```sh
python3 scripts/pi2-image-registry.py list
```

Hardware-passed images live in `~/Documents/Gabix-Images`, outside disposable
Cargo `target` directories. The registry verifies the complete checksum before
use and requires the preserved file to be read-only and user-immutable:

```sh
python3 scripts/pi2-image-registry.py verify \
  pi2-uart-smoke-working-2026-08-09
```

Record the result of every candidate immediately after its physical test. A
passing result requires the tested image bytes; the command verifies, copies,
and protects them before changing the tracked status:

```sh
python3 scripts/pi2-image-registry.py record-result IMAGE_ID \
  --status hardware-passed \
  --evidence "exact UART or LED observation" \
  --image /path/to/tested.img

python3 scripts/pi2-image-registry.py record-result IMAGE_ID \
  --status hardware-failed \
  --evidence "exact UART or LED observation"
```

Never flash an image by an ambiguous build filename. Resolve a registered,
verified artifact and review its id and checksum first:

```sh
python3 scripts/pi2-image-registry.py path IMAGE_ID
```

Removing a protected image requires the deliberate sequence `chflags nouchg`
followed by restoring write permission. Do this only when retiring an artifact
whose checksum and replacement are already recorded.

## Upload development kernels without reflashing

The SD card's development image contains a small UART loader linked at
`0x00008000`. It prints `GABIX LOADER READY` and waits for a complete kernel.
Build the loader once and build each development kernel separately:

```sh
sh scripts/build-arm.sh pi2-loader
sh scripts/build-arm.sh pi2-dev
```

Create the loader SD image from the full official-derived UART smoke image.
This exact source booted on the physical Pi and keeps the complete official
partition layout, firmware, and UART clock configuration fixed:

```sh
sh scripts/make-pi2-loader-image.sh \
  target/official/gabix-official-with-gabix-smoke-clockfix.img \
  target/official/gabix-pi2-loader-full.img
```

The baseline image is a preserved local test artifact rather than a Git file.
Its required SHA-256 is:

```text
66abb107a93d2db17729e7fcc5bedc2c4c6be15c5d2844d0e29d0306e870d5c6
```

Check it before every derived build:

```sh
shasum -a 256 target/official/gabix-official-with-gabix-smoke-clockfix.img
```

If the artifact is missing, download the pinned Raspberry Pi OS Legacy
(32-bit) Lite image and verify the compressed file before decompressing it:

```text
URL: https://downloads.raspberrypi.com/raspios_oldstable_lite_armhf/images/raspios_oldstable_lite_armhf-2026-06-19/2026-06-18-raspios-bookworm-armhf-lite.img.xz
compressed SHA-256: 8a044f4c55feb9b0626ab2060a2eef15c3f57327dd610a0a4cac02cdb959166e
decompressed SHA-256: 494db22e6f0f652e97fe6ed5c89fa51810f2abfa5afe6bcb422c6517d7aacd24
```

Recreate the smoke baseline from the complete decompressed image, the smoke
kernel, and the tracked boot configuration:

```sh
sh scripts/build-pi2-uart-smoke.sh target/pi2-uart-smoke/kernel7.img
python3 scripts/make-pi2-uart-smoke-from-official.py \
  target/official/2026-06-18-raspios-bookworm-armhf-lite.img \
  target/pi2-uart-smoke/kernel7.img \
  target/official/gabix-official-with-gabix-smoke-clockfix.img \
  --config targets/armv7/boot/config.txt
```

The result must have SHA-256
`66abb107a93d2db17729e7fcc5bedc2c4c6be15c5d2844d0e29d0306e870d5c6`.
The 520 MiB `pi2-uart-smoke-fat32-official-layout.img` was an intermediate
layout experiment and must not be used as the physical baseline.

Boot this recreated smoke image on the physical Pi and require
`gabix uart smoke alive` before accepting it as the new baseline. Record its
source-image checksum and resulting baseline checksum in the progress log.

The builder writes a new image file and does not access an SD device. It checks
that `bootcode.bin`, `start.elf`, and `fixup.dat` match the known-good source;
that `kernel7.img` and `config.txt` match the current build; and that the kernel
occupies one contiguous FAT32 cluster chain. It also updates both FAT copies and
the FAT32 free-space records.

### Repeatable SD write procedure

Keep image construction, physical writing, and boot testing as three explicit
steps. Never modify the known-good smoke image in place.

1. Build and verify the image with the command above. Record the printed output
   SHA-256.
2. Insert the SD card and identify it every time:

   ```sh
   diskutil list external physical
   diskutil info /dev/diskN
   ```

   Confirm `Device Location: External`, the expected card capacity, and
   `Media Read-Only: No`. Disk numbers can change after every reconnection.
3. Report the exact `/dev/diskN` target and obtain confirmation before writing.
4. Unmount and write through the raw device:

   ```sh
   diskutil unmountDisk /dev/diskN
   sudo dd \
     if=target/official/gabix-pi2-loader-full.img \
     of=/dev/rdiskN \
     bs=16m conv=sync status=progress
   sync
   ```

5. Verify all boot files after macOS mounts `BOOTFS`:

   ```sh
   python3 scripts/verify-pi2-loader-card.py \
     target/official/gabix-pi2-loader-full.img \
     /Volumes/BOOTFS
   ```

   This compares `bootcode.bin`, `start.elf`, `fixup.dat`, `kernel7.img`, and
   `config.txt` byte-for-byte against the built image.
6. Unmount and check FAT metadata before removing the card:

   ```sh
   diskutil unmountDisk /dev/diskN
   sudo fsck_msdos -n /dev/rdiskNs1
   ```

   The checker must complete all three phases without a free-space mismatch,
   differing FAT copies, broken chain, or orphan cluster.

Do not use a whole-device hash after macOS has mounted the FAT partition as the
acceptance signal. Mounting can update FAT metadata while leaving every boot
file unchanged. Comparing the five boot files gives the useful, fast post-write
signal; the image builder and `fsck_msdos` own the structural FAT checks.

The first loader-image attempt broke this rule by growing the 260-byte smoke
kernel with a fragmented chain (`663 → 3 → 10 → 658 → ...`) and leaving FAT32
free-space metadata stale. In the corrected image observed on 2026-08-15, the
61,580-byte loader occupied contiguous clusters `668–683`; future builds only
require contiguity, not that exact range. The verifier rejects fragmented
output. Raspberry Pi documents four short ACT LED flashes as `start*.elf` not
found; treat that as an SD boot-layout failure before debugging UART or kernel
code. See the [Raspberry Pi LED warning codes](https://www.raspberrypi.com/documentation/configuration/bootfolder.md#led-warning-flash-codes).

The development kernel is linked at `0x00100000`, which is the address where
the loader writes it. The loader accepts at most 16 MiB. It checks the protocol
version, declared size, and CRC32 before executing any received bytes.

Only one process may own the serial device. Stop the passive monitor, upload,
then restart the monitor if required:

```sh
sh scripts/monitor-pi2-uart.sh stop
python3 scripts/upload-pi2-kernel.py \
  /dev/cu.usbserial-DEVICE \
  targets/armv7/target/armv7a-none-eabihf/debug/kernel-pi2-dev.img
sh scripts/monitor-pi2-uart.sh start /dev/cu.usbserial-DEVICE
```

The uploader can attach after the loader printed its ready line. It sends the
marker directly, displays bytes sent, percentage, transfer rate, and ETA, then
waits for `UPLOAD OK` or `UPLOAD FAILED`. It rejects an already-owned UART and
retries the complete image up to three times after a loader rejection. Add
`--wait-ready` when the uploader already owns the UART before the Pi resets. A
valid upload starts immediately. A five-second payload inter-byte timeout,
invalid header, or bad CRC returns the loader to its ready state.

Gabix system call 169 requests a platform reboot with no arguments. On the Pi,
the kernel triggers the BCM2836 power-manager watchdog reset. Firmware then
loads the UART loader from the SD card again. A physical power cycle remains the
fallback when a broken development kernel cannot invoke the system call.

The `block read` line appears only when sector 1 contains the QEMU test marker,
so the physical SD image does not print it.

## Diagnose the first physical boot

| Last observed result | Check next |
|---|---|
| No boot output | Rebuild and verify the image; check the SD card and serial wiring |
| `booted`, then no `memory mapped` | ARM page-table setup and the serial log after that line |
| `heap allocated`, then no `file read` | SDHCI initialization, MBR partition lookup, and ext2 reads |
| `file read`, then no `usb enumerated` | Firmware USB power request, DWC2 reset, FIFO setup, and root-port power |
| `usb enumerated`, then no `usb network configured` | Hub child enumeration, LAN9512 USB identity, and LAN9512 register initialization |
| `usb network configured`, then no DHCP lease | Ethernet cable, router port, LAN9512 PHY/link, and DHCP traffic |
| DHCP lease exists, then no ping reply | LAN9512 receive and transmit framing plus router ARP state |
| Ping works, then no UDP echo | UDP destination port 5555 and host firewall behavior |

## What QEMU verifies

The external behavioral suite boots QEMU's `raspi2b` machine and verifies ARM
startup, memory mapping, heap allocation, timer interrupts, SD reads, MBR and
ext2 access, user mode, system calls, USB host transfers, DHCP, and UDP echo.
QEMU connects the emulated DWC2 controller through its emulated USB hub to an
RNDIS Ethernet device.

The hardware build also compiles the Raspberry Pi firmware mailbox calls,
DWC2 controller initialization, DMA bus addresses, and LAN9512 driver. A real
Pi is required to verify the mailbox response, DWC2 timing, hub port behavior,
LAN9512 vendor registers, EEPROM MAC read, PHY link, and Ethernet traffic.

Raspberry Pi documents the boot settings and kernel filename rules in its
[config.txt documentation](https://www.raspberrypi.com/documentation/computers/config_txt.html)
and the board UART and GPIO interfaces in its
[computer configuration documentation](https://www.raspberrypi.com/documentation/configuration/computers/raspberry-pi.html).
