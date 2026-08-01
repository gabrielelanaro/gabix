#!/usr/bin/env python3

import struct
import sys
from pathlib import Path

SECTOR_SIZE = 512
REQUIRED_BOOT_FILES = {
    b"BOOTCODEBIN",
    b"START   ELF",
    b"FIXUP   DAT",
    b"KERNEL7 IMG",
    b"CONFIG  TXT",
}


def partition(mbr: bytes, index: int) -> tuple[int, int, int]:
    entry = 446 + index * 16
    return (
        mbr[entry + 4],
        struct.unpack_from("<I", mbr, entry + 8)[0],
        struct.unpack_from("<I", mbr, entry + 12)[0],
    )


def verify(image: Path) -> None:
    with image.open("rb") as disk:
        mbr = disk.read(SECTOR_SIZE)
        if mbr[510:512] != b"\x55\xaa":
            raise ValueError("missing MBR signature")
        boot_type, boot_start, _ = partition(mbr, 0)
        root_type, root_start, _ = partition(mbr, 1)
        if boot_type != 0x0E or root_type != 0x83:
            raise ValueError("unexpected partition types")

        disk.seek(boot_start * SECTOR_SIZE)
        boot = disk.read(SECTOR_SIZE)
        bytes_per_sector = struct.unpack_from("<H", boot, 11)[0]
        reserved = struct.unpack_from("<H", boot, 14)[0]
        fats = boot[16]
        root_entries = struct.unpack_from("<H", boot, 17)[0]
        fat_sectors = struct.unpack_from("<H", boot, 22)[0]
        root_sector = boot_start + reserved + fats * fat_sectors
        disk.seek(root_sector * bytes_per_sector)
        directory = disk.read(root_entries * 32)
        names = {
            directory[offset : offset + 11]
            for offset in range(0, len(directory), 32)
            if directory[offset] not in (0, 0xE5)
        }
        missing = REQUIRED_BOOT_FILES - names
        if missing:
            raise ValueError(f"missing boot files: {sorted(missing)}")

        disk.seek(root_start * SECTOR_SIZE + 1080)
        if disk.read(2) != b"\x53\xef":
            raise ValueError("missing ext2 root filesystem")


if __name__ == "__main__":
    verify(Path(sys.argv[1]))
    print(sys.argv[1])
