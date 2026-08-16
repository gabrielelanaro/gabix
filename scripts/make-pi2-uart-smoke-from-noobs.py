#!/usr/bin/env python3

import gzip
import hashlib
import math
import os
import struct
import sys
from pathlib import Path

SECTOR_SIZE = 512
KNOWN_BOOTCODE_SHA256 = "cb1d96c9833afc62d502128f3520efa9df63f2ad881143974f60cea06356496f"


def set_fat32_entry(image: bytearray, fat_offsets: list[int], cluster: int, value: int) -> None:
    for fat_offset in fat_offsets:
        struct.pack_into("<I", image, fat_offset + cluster * 4, value)


def create(
    backup: Path,
    firmware_dir: Path,
    kernel: Path,
    config: Path,
    output: Path,
) -> None:
    with gzip.open(backup, "rb") as compressed:
        image = bytearray(compressed.read())

    if len(image) < 16 * 1024 * 1024:
        raise ValueError("the NOOBS backup does not contain enough boot-partition data")
    if image[510:512] != b"\x55\xaa":
        raise ValueError("the NOOBS backup has no MBR signature")

    partition_entry = 446
    partition_start = struct.unpack_from("<I", image, partition_entry + 8)[0]
    boot_offset = partition_start * SECTOR_SIZE
    boot = image[boot_offset : boot_offset + SECTOR_SIZE]
    bytes_per_sector = struct.unpack_from("<H", boot, 11)[0]
    sectors_per_cluster = boot[13]
    reserved_sectors = struct.unpack_from("<H", boot, 14)[0]
    fat_count = boot[16]
    fat_sectors = struct.unpack_from("<I", boot, 36)[0]
    root_cluster = struct.unpack_from("<I", boot, 44)[0]
    fsinfo_sector = struct.unpack_from("<H", boot, 48)[0]
    backup_boot_sector = struct.unpack_from("<H", boot, 50)[0]
    total_sectors = struct.unpack_from("<I", boot, 32)[0]

    if (
        bytes_per_sector != SECTOR_SIZE
        or sectors_per_cluster == 0
        or fat_count != 2
        or fat_sectors == 0
        or root_cluster != 2
        or boot[82:90] != b"FAT32   "
    ):
        raise ValueError("the backup does not have the expected NOOBS FAT32 geometry")

    fat_offsets = [
        (partition_start + reserved_sectors + index * fat_sectors) * SECTOR_SIZE
        for index in range(fat_count)
    ]
    data_offset = (
        partition_start + reserved_sectors + fat_count * fat_sectors
    ) * SECTOR_SIZE
    cluster_size = sectors_per_cluster * SECTOR_SIZE
    root_offset = data_offset + (root_cluster - 2) * cluster_size
    original_root = image[root_offset : root_offset + cluster_size]

    bootcode = None
    bootcode_first_cluster = None
    for index in range(0, len(original_root) // 32):
        entry = original_root[index * 32 : (index + 1) * 32]
        if entry[0] == 0:
            break
        if entry[0] == 0xE5 or entry[11] == 0x0F:
            continue
        name = bytes(entry[:11])
        first_cluster = (
            struct.unpack_from("<H", entry, 20)[0] << 16
        ) | struct.unpack_from("<H", entry, 26)[0]
        size = struct.unpack_from("<I", entry, 28)[0]
        if name == b"BOOTCODEBIN":
            offset = data_offset + (first_cluster - 2) * cluster_size
            bootcode = bytes(image[offset : offset + size])
            bootcode_first_cluster = first_cluster

    if bootcode is None or bootcode_first_cluster is None:
        raise ValueError("the known-good NOOBS bootcode.bin was not found")
    if hashlib.sha256(bootcode).hexdigest() != KNOWN_BOOTCODE_SHA256:
        raise ValueError("the NOOBS bootcode.bin does not match the known-good card")
    downloaded_bootcode = (firmware_dir / "bootcode.bin").read_bytes()
    if downloaded_bootcode != bootcode:
        raise ValueError("the downloaded firmware revision does not match the backup")

    files = [
        (b"START   ELF", (firmware_dir / "start.elf").read_bytes()),
        (b"FIXUP   DAT", (firmware_dir / "fixup.dat").read_bytes()),
        (b"KERNEL7 IMG", kernel.read_bytes()),
        (b"CONFIG  TXT", config.read_bytes()),
    ]
    if b"gabix uart smoke booted" not in files[2][1]:
        raise ValueError("the supplied kernel is not the UART smoke kernel")

    fat_entry_count = fat_sectors * SECTOR_SIZE // 4
    for cluster in range(2, fat_entry_count):
        set_fat32_entry(image, fat_offsets, cluster, 0)
    set_fat32_entry(image, fat_offsets, root_cluster, 0x0FFFFFFF)

    root = bytearray(cluster_size)
    bootcode_cluster_count = math.ceil(len(bootcode) / cluster_size)
    for index in range(bootcode_cluster_count):
        cluster = bootcode_first_cluster + index
        following = 0x0FFFFFFF if index == bootcode_cluster_count - 1 else cluster + 1
        set_fat32_entry(image, fat_offsets, cluster, following)
    bootcode_entry = bytearray(32)
    bootcode_entry[:11] = b"BOOTCODEBIN"
    bootcode_entry[11] = 0x20
    struct.pack_into("<H", bootcode_entry, 20, bootcode_first_cluster >> 16)
    struct.pack_into("<H", bootcode_entry, 26, bootcode_first_cluster & 0xFFFF)
    struct.pack_into("<I", bootcode_entry, 28, len(bootcode))
    root[:32] = bootcode_entry

    label = bytearray(32)
    label[:11] = b"UARTBOOT   "
    label[11] = 0x08
    root[32:64] = label

    next_cluster = bootcode_first_cluster + bootcode_cluster_count
    for root_index, (name, contents) in enumerate(files, start=2):
        cluster_count = max(1, math.ceil(len(contents) / cluster_size))
        first_cluster = next_cluster
        for index in range(cluster_count):
            cluster = first_cluster + index
            following = 0x0FFFFFFF if index == cluster_count - 1 else cluster + 1
            set_fat32_entry(image, fat_offsets, cluster, following)
        file_offset = data_offset + (first_cluster - 2) * cluster_size
        allocated_size = cluster_count * cluster_size
        image[file_offset : file_offset + allocated_size] = contents.ljust(allocated_size, b"\0")

        entry = bytearray(32)
        entry[:11] = name
        entry[11] = 0x20
        struct.pack_into("<H", entry, 20, first_cluster >> 16)
        struct.pack_into("<H", entry, 26, first_cluster & 0xFFFF)
        struct.pack_into("<I", entry, 28, len(contents))
        root[root_index * 32 : (root_index + 1) * 32] = entry
        next_cluster += cluster_count

    image[root_offset : root_offset + cluster_size] = root
    for sector in (0, backup_boot_sector):
        label_offset = boot_offset + sector * SECTOR_SIZE + 71
        image[label_offset : label_offset + 11] = b"UARTBOOT   "

    data_clusters = (
        total_sectors - reserved_sectors - fat_count * fat_sectors
    ) // sectors_per_cluster
    allocated_clusters = 1 + bootcode_cluster_count + sum(
        max(1, math.ceil(len(contents) / cluster_size)) for _, contents in files
    )
    free_clusters = data_clusters - allocated_clusters
    for sector in (fsinfo_sector, backup_boot_sector + fsinfo_sector):
        fsinfo_offset = boot_offset + sector * SECTOR_SIZE
        struct.pack_into("<I", image, fsinfo_offset + 488, free_clusters)
        struct.pack_into("<I", image, fsinfo_offset + 492, next_cluster)

    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("wb") as destination:
        destination.write(image)
        destination.flush()
        os.fsync(destination.fileno())
    print(output)
    print(hashlib.sha256(image).hexdigest())


if __name__ == "__main__":
    if len(sys.argv) != 6:
        raise SystemExit(
            "usage: make-pi2-uart-smoke-from-noobs.py "
            "<backup.img.gz> <firmware-dir> <kernel7.img> <config.txt> <output.img>"
        )
    create(*(Path(argument) for argument in sys.argv[1:]))
