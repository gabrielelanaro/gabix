# Send a file from the Mac to the Pi's SD card

The `pi2-sd-transfer` kernel receives a file over the existing Ethernet link and
stores it in a fixed reserved area of the registered recovery image. It keeps
UART uploads, DHCP, ping, and UDP echo available. Boot selection is unchanged.
This is an opt-in development service for the private direct link; it has no
network authentication and supports one uploader at a time.

## Run it

Start the Mac's existing DHCP peer on `en6`, then build and upload the kernel
through the UART loader:

```sh
GABIX_ARM_PROFILE=release sh scripts/build-arm.sh pi2-sd-transfer
python3 scripts/upload-pi2-kernel.py /dev/cu.usbserial-0001 \
  targets/armv7/target/armv7a-none-eabihf/release/kernel-pi2-sd-transfer.img \
  --inter-byte-delay-us 500 --retries 1 --wait-for-user-ready --user-ready-timeout 120
python3 scripts/pi2-sd-transfer.py path/to/file.bin
```

The uploader waits for every chunk acknowledgement, requests a whole-file
checksum check on the Pi, then downloads every stored byte and compares it with
the local file. It reports success only after that comparison. The Pi buffers
one sector at a time rather than keeping the whole file in RAM.

After a UART-triggered reboot and re-upload of the kernel, verify persistence
without uploading the file again:

```sh
python3 scripts/pi2-sd-transfer.py path/to/file.bin --verify-only
```

## Card area and checks

Only the known recovery image is accepted: disk signature and all MBR partition
entries must match. On first use, every sector in the reserved area must be zero.
The saved original recovery image has a zero-filled gap before the first
partition, which begins at sector 16384.

- Sector 8192: file header (`GABIXNET`, byte length, file CRC32, completion flag,
  header CRC32; remaining bytes zero).
- Sectors 8193–12287: file bytes, at most 2,096,640 bytes. The final sector is
  zero-padded; the file length excludes padding.
- Partition contents and boot files are untouched.

A BEGIN writes an incomplete header before replacing the stored file. FINISH
reads the stored file and validates its CRC before marking it complete. A fresh
kernel can reopen a completed file. An I/O error disables this receiver until
reboot. An incomplete transfer can be replaced by a new BEGIN after restart;
an invalid nonzero header causes initialization to refuse the region.

This area stores one staging file. Replacing it discards the previous staging
file, while the SD-resident UART loader remains the boot path. The header is
not a boot pointer. Power-loss guarantees, signed images, kernel activation,
and fallback selection remain future work. The earlier standalone SD sector
test deliberately refuses its zero-sector precondition once sector 8192 holds
this file header; it must not be used to erase the staged file.

## Small UDP protocol

UDP port 5556. Each packet starts with 20 bytes: `GBSD`, operation, session ID,
argument, value. The four integers are little-endian 32-bit values. Session IDs
must be nonzero; an active transfer is associated with the sender IP and UDP
port. The host uses a random session ID and stop-and-wait retries.

| Operation | Argument / value | Payload |
|---|---|---|
| 1 BEGIN | file length / file CRC32 | empty |
| 2 DATA | byte offset / chunk CRC32 | 512 bytes, or exact final remainder |
| 3 FINISH | zero / zero | empty |
| 4 READ | aligned byte offset / zero | empty |
| 5 OPEN | zero / zero | empty |

Replies set the operation's high bit, echo session and argument, and use value
as status: 0 success, 1 invalid, 2 out of order, 3 I/O error, 4 checksum mismatch.
READ success adds the stored chunk. OPEN success adds length and file CRC32.
Duplicate DATA is compared against the stored sector and acknowledged without
another write. Future offsets are rejected. Every accepted new chunk is written
and read back before its acknowledgement. The sender never chooses an SD LBA.

## Physical verification

One behavioral test covers a 65,549-byte file, a partial final sector, duplicate
chunks/finish, out-of-order rejection, corrupt-chunk rejection, and exact read-back:

```sh
python3 tests/test_ethernet_stores_file.py
# After UART reboot/re-upload:
python3 tests/test_ethernet_stores_file.py --verify-only
```

Keep serial captures and test payloads under ignored `target/runs/`.
