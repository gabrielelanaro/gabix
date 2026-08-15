# Raspberry Pi 2 UART loader milestone

The Gabix stage-one UART loader now boots on the physical Raspberry Pi 2B.
The image is preserved separately from disposable build output.

## Verified artifact

- Registry id: `pi2-uart-loader-full-2026-08-15`
- Durable path: `~/Documents/Gabix-Images/gabix-pi2-loader-full.img`
- Size: `2,499,805,184` bytes
- SHA-256:
  `cdd2f94825edea97b865dd002a49e8558bcf3dbb43657f6b5074fc02054d7183`
- Registry status: `hardware-passed`
- Protection: read-only permissions plus macOS user-immutable flag (`uchg`)

## Reproducible steps

1. Verify the protected smoke baseline:

   ```sh
   python3 scripts/pi2-image-registry.py verify \
     pi2-uart-smoke-working-2026-08-09
   ```

2. Build the Pi 2 loader and derive a new image from the complete working
   smoke image. The source must have SHA-256
   `66abb107a93d2db17729e7fcc5bedc2c4c6be15c5d2844d0e29d0306e870d5c6`:

   ```sh
   sh scripts/make-pi2-loader-image.sh \
     target/official/gabix-official-with-gabix-smoke-clockfix.img \
     target/official/gabix-pi2-loader-full.img
   ```

3. Verify the candidate image and identify the external SD device. The tested
   device was `/dev/disk7` / `/dev/rdisk7`:

   ```sh
   loader_kernel=$(sh scripts/build-arm.sh pi2-loader | tail -1)
   python3 scripts/verify-pi2-loader-image.py \
     target/official/gabix-pi2-loader-full.img "$loader_kernel" \
     targets/armv7/boot/config.txt \
     --source ~/Documents/Gabix-Images/gabix-pi2-uart-smoke-working-2026-08-09.img
   diskutil list external physical
   ```

4. Unmount and write the complete image with a progress display:

   ```sh
   diskutil unmountDisk /dev/disk7
   sudo dd if=target/official/gabix-pi2-loader-full.img \
     of=/dev/rdisk7 bs=16m conv=sync status=progress
   sync
   ```

5. Mount the card and compare `BOOTCODE.BIN`, `START.ELF`, `FIXUP.DAT`,
   `KERNEL7.IMG`, and `CONFIG.TXT` byte-for-byte against the image. All five
   comparisons passed in this test.

6. Unmount the card, start exactly one UART monitor at 115200 baud, insert the
   card into the Pi, and power-cycle it:

   ```sh
   sh scripts/monitor-pi2-uart.sh stop
   sh scripts/monitor-pi2-uart.sh start /dev/cu.usbserial-0001
   tail -f /path/from-the-monitor-output/screenlog.0
   ```

## Physical result

The sole UART monitor captured this exact line after power-on:

```text
GABIX LOADER READY
```

This proves the Pi firmware found the complete official boot layout, loaded
`KERNEL7.IMG`, entered the Gabix loader, and initialized UART at 115200 baud.
The next experiment is a UART kernel upload; it does not require another SD
card write.
