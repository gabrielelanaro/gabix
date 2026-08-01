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

## Optional serial diagnostics

Use a 3.3 V USB-to-TTL serial adapter. Connect these pins while the Pi is
powered off:

| Raspberry Pi header | Serial adapter |
|---|---|
| Pin 6, ground | Ground |
| Pin 8, GPIO14 / Pi TX | Adapter RX |
| Pin 10, GPIO15 / Pi RX | Adapter TX, optional |

Do not connect the adapter's 5 V or 3.3 V power pin. Configure the terminal for
115200 baud, 8 data bits, no parity, and 1 stop bit. On macOS, a typical command
is:

```sh
screen /dev/cu.usbserial-DEVICE 115200
```

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
