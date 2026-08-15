#!/usr/bin/env python3

import struct
import sys
from pathlib import Path

SECTOR_SIZE = 512


def verify(image: Path) -> None:
    with image.open("rb") as disk:
        mbr = disk.read(SECTOR_SIZE)
        if mbr[510:512] != b"\x55\xaa":
            raise ValueError("missing MBR signature")
        if mbr[446 + 4] != 0x0E:
            raise ValueError("missing FAT16 boot partition")
        boot_start = struct.unpack_from("<I", mbr, 446 + 8)[0]
        boot_size = struct.unpack_from("<I", mbr, 446 + 12)[0]

        disk.seek(boot_start * SECTOR_SIZE)
        boot = disk.read(SECTOR_SIZE)
        if boot[510:512] != b"\x55\xaa":
            raise ValueError("missing FAT boot signature")
        bytes_per_sector = struct.unpack_from("<H", boot, 11)[0]
        reserved = struct.unpack_from("<H", boot, 14)[0]
        fats = boot[16]
        root_entries = struct.unpack_from("<H", boot, 17)[0]
        fat_sectors = struct.unpack_from("<H", boot, 22)[0]
        sectors_per_cluster = boot[13]
        if bytes_per_sector != SECTOR_SIZE or sectors_per_cluster == 0:
            raise ValueError("invalid FAT geometry")

        root_sector = boot_start + reserved + fats * fat_sectors
        disk.seek(root_sector * bytes_per_sector)
        directory = disk.read(root_entries * 32)
        entries = {}
        for offset in range(0, len(directory), 32):
            entry = directory[offset : offset + 32]
            if entry[0] in (0, 0xE5):
                continue
            name = entry[:11]
            first_cluster = struct.unpack_from("<H", entry, 26)[0]
            size = struct.unpack_from("<I", entry, 28)[0]
            entries[name] = (first_cluster, size)

        required = {
            b"BOOTCODEBIN",
            b"START   ELF",
            b"FIXUP   DAT",
            b"KERNEL7 IMG",
            b"CONFIG  TXT",
        }
        missing = required - entries.keys()
        if missing:
            raise ValueError(f"missing boot files: {sorted(missing)}")

        fat_start = (boot_start + reserved) * bytes_per_sector
        data_start = (
            boot_start + reserved + fats * fat_sectors + root_entries * 32 // bytes_per_sector
        ) * bytes_per_sector
        fat_bytes = fat_sectors * bytes_per_sector
        disk.seek(fat_start)
        fat = disk.read(fat_bytes)
        cluster_size = sectors_per_cluster * bytes_per_sector
        for name in required:
            first_cluster, size = entries[name]
            if first_cluster < 2:
                raise ValueError(f"{name!r} has no data cluster")
            cluster_count = max(1, (size + cluster_size - 1) // cluster_size)
            for index in range(cluster_count):
                cluster = first_cluster + index
                next_cluster = struct.unpack_from("<H", fat, cluster * 2)[0]
                expected = 0xFFFF if index == cluster_count - 1 else cluster + 1
                if next_cluster != expected:
                    raise ValueError(f"{name!r} has a broken FAT chain")
            disk.seek(data_start + (first_cluster - 2) * cluster_size)
            data = disk.read(size)
            if len(data) != size:
                raise ValueError(f"{name!r} is truncated")
            if name == b"KERNEL7 IMG" and b"gabix uart smoke booted" not in data:
                raise ValueError("smoke kernel marker is missing")

        if boot_size * SECTOR_SIZE != 131072 * SECTOR_SIZE:
            raise ValueError("unexpected FAT16 partition size")


if __name__ == "__main__":
    verify(Path(sys.argv[1]))
    print(sys.argv[1])
