#!/usr/bin/env python3

import math
import os
import struct
import sys
from pathlib import Path

SECTOR_SIZE = 512
TOTAL_SECTORS = 131_072
SECTORS_PER_CLUSTER = 4
RESERVED_SECTORS = 1
FAT_COUNT = 2
ROOT_ENTRIES = 512
ROOT_SECTORS = ROOT_ENTRIES * 32 // SECTOR_SIZE


def fat_name(name: str) -> bytes:
    base, separator, extension = name.upper().partition(".")
    if len(base) > 8 or len(extension) > 3 or (separator and not extension):
        raise ValueError(f"{name} is not an 8.3 filename")
    return base.ljust(8).encode("ascii") + extension.ljust(3).encode("ascii")


def create(output: Path, hidden_sectors: int, specifications: list[str]) -> None:
    files: list[tuple[bytes, Path, int]] = []
    for specification in specifications:
        name, source_text = specification.split("=", 1)
        source = Path(source_text)
        files.append((fat_name(name), source, source.stat().st_size))

    fat_sectors = 1
    while True:
        data_sectors = (
            TOTAL_SECTORS
            - RESERVED_SECTORS
            - FAT_COUNT * fat_sectors
            - ROOT_SECTORS
        )
        clusters = data_sectors // SECTORS_PER_CLUSTER
        required = math.ceil((clusters + 2) * 2 / SECTOR_SIZE)
        if required == fat_sectors:
            break
        fat_sectors = required
    if not 4085 <= clusters < 65525:
        raise ValueError("volume parameters do not describe FAT16")

    boot = bytearray(SECTOR_SIZE)
    boot[0:3] = b"\xeb\x3c\x90"
    boot[3:11] = b"GABIX   "
    struct.pack_into("<H", boot, 11, SECTOR_SIZE)
    boot[13] = SECTORS_PER_CLUSTER
    struct.pack_into("<H", boot, 14, RESERVED_SECTORS)
    boot[16] = FAT_COUNT
    struct.pack_into("<H", boot, 17, ROOT_ENTRIES)
    struct.pack_into("<H", boot, 19, 0)
    boot[21] = 0xF8
    struct.pack_into("<H", boot, 22, fat_sectors)
    struct.pack_into("<H", boot, 24, 32)
    struct.pack_into("<H", boot, 26, 64)
    struct.pack_into("<I", boot, 28, hidden_sectors)
    struct.pack_into("<I", boot, 32, TOTAL_SECTORS)
    boot[36] = 0x80
    boot[38] = 0x29
    struct.pack_into("<I", boot, 39, 0x47414258)
    boot[43:54] = b"GABIX BOOT "
    boot[54:62] = b"FAT16   "
    boot[510:512] = b"\x55\xaa"

    fat = bytearray(fat_sectors * SECTOR_SIZE)
    struct.pack_into("<HH", fat, 0, 0xFFF8, 0xFFFF)
    root = bytearray(ROOT_SECTORS * SECTOR_SIZE)
    cluster_size = SECTORS_PER_CLUSTER * SECTOR_SIZE
    next_cluster = 2
    allocations: list[tuple[Path, int, int]] = []
    for index, (name, source, size) in enumerate(files):
        cluster_count = max(1, math.ceil(size / cluster_size))
        first_cluster = next_cluster
        for cluster in range(first_cluster, first_cluster + cluster_count):
            following = 0xFFFF if cluster == first_cluster + cluster_count - 1 else cluster + 1
            struct.pack_into("<H", fat, cluster * 2, following)
        entry = index * 32
        root[entry : entry + 11] = name
        root[entry + 11] = 0x20
        struct.pack_into("<H", root, entry + 26, first_cluster)
        struct.pack_into("<I", root, entry + 28, size)
        allocations.append((source, first_cluster, size))
        next_cluster += cluster_count

    fat_start = RESERVED_SECTORS * SECTOR_SIZE
    root_start = (RESERVED_SECTORS + FAT_COUNT * fat_sectors) * SECTOR_SIZE
    data_start = root_start + len(root)
    with output.open("wb") as volume:
        volume.truncate(TOTAL_SECTORS * SECTOR_SIZE)
        volume.seek(0)
        volume.write(boot)
        for index in range(FAT_COUNT):
            volume.seek(fat_start + index * len(fat))
            volume.write(fat)
        volume.seek(root_start)
        volume.write(root)
        for source, first_cluster, _ in allocations:
            offset = data_start + (first_cluster - 2) * cluster_size
            volume.seek(offset)
            with source.open("rb") as input_file:
                while chunk := input_file.read(1024 * 1024):
                    volume.write(chunk)
        volume.flush()
        os.fsync(volume.fileno())


if __name__ == "__main__":
    create(Path(sys.argv[1]), int(sys.argv[2], 0), sys.argv[3:])
