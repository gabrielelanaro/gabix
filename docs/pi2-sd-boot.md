# Raspberry Pi SD boot and Ethernet updates

The Pi firmware reads `KERNEL7.IMG` from the existing FAT boot partition.
That file contains our loader. The loader reads a boot record from the new
kernel partition, copies the selected kernel into RAM at `0x00100000`, checks
its CRC32, and branches to its entry point.

## Storage

The physical card has 15,523,840 sectors of 512 bytes (7,948,206,080 bytes).
The original FAT and Linux partitions retain their locations and contents,
except for the FAT loader file and its directory size field.

| Region | First sector | Size |
|---|---:|---:|
| New partition, MBR type `0xda` | 4,882,432 | 128 MiB |
| Boot record and reserved metadata | 4,882,432 | 1 MiB |
| Kernel A | 4,884,480 | 32 MiB |
| Kernel B | 4,950,016 | 32 MiB |
| Future allocation | 5,015,552 | 63 MiB |

The record stores each copy's relative start, capacity, byte length, and CRC32.
The current executable limit is **16 MiB**, matching the UART loader's RAM
contract. Larger kernels require a separate review of stacks, user allocations,
and loading addresses. Storage allocation alone does not change those addresses.

## Selecting and confirming a kernel

`crates/kernel-core/src/boot_record.rs` defines the record and selection rules.
`targets/armv7/src/pi2_loader.rs` performs the reads and executes the selection.

1. An Ethernet upload writes the inactive copy. Its length is cleared before
   the first data write, so an interrupted upload cannot be selected.
2. Each chunk is read back before acknowledgement. Completion checks the whole
   image CRC and records the copy as pending.
3. On reboot the loader tries pending, then active, then the other copy. Each
   attempt is recorded on SD before execution.
4. The loader arms a 15-second watchdog. A kernel that hangs before confirming
   is skipped on the next reset because its attempt was already recorded.
5. The development kernel refreshes that watchdog during its bounded startup
   tests. Once its network and SD services are initialized and its UART service
   is ready, it confirms the boot, clears the attempted flags, and becomes active.
6. If neither copy is usable, the loader waits for a kernel over UART.

`targets/armv7/src/boot_trial.rs` validates the loader's RAM handoff and writes
confirmation. The confirmation threshold establishes that the development
services started; it cannot prove the absence of later software bugs.

The loader also provides a two-second UART upload window before SD boot.
The watchdog protects unconfirmed startup. A confirmed running kernel has no
continuous health monitor in this implementation.

## Updating over Ethernet

Build and send a development kernel while the Pi's service is running:

```sh
GABIX_ARM_PROFILE=release sh scripts/build-arm.sh pi2-sd-transfer
python3 scripts/pi2-sd-transfer.py \
  targets/armv7/target/armv7a-none-eabihf/release/kernel-pi2-sd-transfer.img \
  --kernel
python3 scripts/upload-pi2-user.py /dev/cu.usbserial-0001 \
  target/arm_reboot.elf --wait-for-reboot
```

The host reads the complete uploaded image back for comparison. The reboot
starts the pending copy; confirmation makes it active. The previous copy stays
on the card. Transfers use the existing private direct Ethernet connection,
UDP port 5556, and DHCP address `10.0.2.15` by default.

`BEGIN_KERNEL` (operation 9) selects the inactive extent using the boot record.
The host cannot supply a destination sector through that operation. Normal file
uploads continue to use the smaller staging area before the FAT partition.

## Initial installation and recovery image

`scripts/pi2-install.py prepare` clones the protected original image into a new
burnable image and prepares two packages: storage and loader. It reads the live
card to include expected old bytes for every changed sector.

`apply` transfers a package into staging, asks the kernel to preflight every
address and expected value, then writes and reads back each changed sector.
Each one-off installation package accepts up to 1,000 changed sectors; routine
`--kernel` uploads stream directly to the inactive copy up to the 16 MiB limit.
The installer accepts only the agreed kernel areas, boot record, third MBR
entry, and the existing FAT allocation of `KERNEL7.IMG`. It preserves the
confirmed kernel when updating kernel data. Boot-loader replacement uses the
existing FAT chain and changes its directory size without allocating clusters.
The baseline image and live card can allocate that file at different clusters;
verification follows each chain and compares the logical file bytes.

Apply storage first, test the candidate loader in RAM, then apply the loader
package. `pi2-loader-trial` embeds the candidate loader in a UART payload and
copies it to its normal RAM address without modifying the installed boot file.

The physical checks are in `tests/test_pi2_sd_boot.py`: hung candidate fallback,
successful candidate confirmation, and two invalid copies followed by UART
recovery. Each requires explicit invocation and writes artifacts under a caller
selected ignored run directory. Run the complete sequence to finish with valid
kernels restored:

```sh
python3 tests/test_pi2_sd_boot.py all --output target/runs/sd-boot-check
```

Use a fresh output directory for each run. Add `--ram` before installing the
loader; that mode re-uploads the RAM trial after a watchdog reset.

## Limits

The single boot record has a CRC, with no redundant copy. Power loss during its
write can require UART recovery. Power loss while replacing the FAT loader can
require burning the preserved recovery image from another computer. The initial
loader installation is therefore a controlled one-off operation.

CRC32 detects accidental corruption; uploads and installation packages have no
authentication. This development service belongs on the private direct link.
The image builder preserves the original firmware and Linux filesystem from
the protected baseline. Image verification checks its loader, both kernel
copies, boot record, and full-file SHA256; it does not substitute for a full
power-loss or reflash test.

## Boot record bytes

All integers are little endian. The record occupies one 512-byte sector.

| Offset | Field |
|---:|---|
| 0 | `GABIXBT1` (8 bytes) |
| 8 | Version, u32 = 1 |
| 12 | Confirmed active copy, u32 = 0 or 1 |
| 16, 40 | Copy descriptor: relative sector u64, capacity sectors u64, image bytes u32, CRC32 u32 |
| 64 | Pending copy u32, or `0xffffffff` |
| 68 | Attempted copies bit mask u32 |
| 72 | Currently attempted copy u32, or `0xffffffff` |
| 508 | CRC32 of bytes 0–507 |

The RAM handoff at `0x00060000` identifies the selected copy and boot-record
sector. The kernel checks the handoff checksum and the loaded image's checksum
before confirming, which prevents a different UART upload from acknowledging
an earlier SD attempt.
