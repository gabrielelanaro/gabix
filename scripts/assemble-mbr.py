#!/usr/bin/env python3

import os
import struct
import sys
from pathlib import Path

SECTOR_SIZE = 512
PARTITION_TABLE_OFFSET = 446


def assemble(output: Path, specifications: list[str]) -> None:
    partitions: list[tuple[int, int, Path, int]] = []
    for specification in specifications:
        type_text, start_text, image_text = specification.split(":", 2)
        image = Path(image_text)
        byte_length = image.stat().st_size
        sectors = (byte_length + SECTOR_SIZE - 1) // SECTOR_SIZE
        partitions.append((int(type_text, 0), int(start_text, 0), image, sectors))

    if not partitions or len(partitions) > 4:
        raise ValueError("provide one to four partitions")
    total_sectors = max(start + sectors for _, start, _, sectors in partitions)
    with output.open("wb") as disk:
        disk.truncate(total_sectors * SECTOR_SIZE)
        mbr = bytearray(SECTOR_SIZE)
        for index, (partition_type, start, _, sectors) in enumerate(partitions):
            entry = PARTITION_TABLE_OFFSET + index * 16
            mbr[entry] = 0x80 if index == 0 else 0
            mbr[entry + 4] = partition_type
            struct.pack_into("<II", mbr, entry + 8, start, sectors)
        mbr[510:512] = b"\x55\xaa"
        disk.seek(0)
        disk.write(mbr)
        for _, start, image, _ in partitions:
            disk.seek(start * SECTOR_SIZE)
            with image.open("rb") as source:
                while chunk := source.read(1024 * 1024):
                    disk.write(chunk)
        disk.flush()
        os.fsync(disk.fileno())


if __name__ == "__main__":
    assemble(Path(sys.argv[1]), sys.argv[2:])
