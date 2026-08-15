#!/usr/bin/env python3
"""Derive an official Raspberry Pi image with Gabix boot files replaced."""

from __future__ import annotations

import argparse
import hashlib
import mmap
from pathlib import Path
import struct
import time


SECTOR_SIZE = 512
FAT32_EOF = 0x0FFFFFF8


def fat_chain(fat: bytes, start: int) -> list[int]:
    clusters: list[int] = []
    cluster = start
    while 2 <= cluster < FAT32_EOF and cluster not in clusters:
        clusters.append(cluster)
        next_cluster = struct.unpack_from("<I", fat, cluster * 4)[0] & 0x0FFFFFFF
        if next_cluster >= FAT32_EOF:
            return clusters
        cluster = next_cluster
    raise ValueError("broken FAT32 cluster chain")


def locate_file(
    image: mmap.mmap, short_name: bytes
) -> tuple[int, int, int, int, list[int]]:
    if len(short_name) != 11:
        raise ValueError("FAT short name must be exactly 11 bytes")
    mbr = image[:SECTOR_SIZE]
    if mbr[510:512] != b"\x55\xaa":
        raise ValueError("missing MBR signature")
    partition = mbr[446:462]
    if partition[4] not in (0x0B, 0x0C, 0x0E):
        raise ValueError("expected FAT partition")

    partition_start = struct.unpack_from("<I", partition, 8)[0]
    boot_offset = partition_start * SECTOR_SIZE
    boot = image[boot_offset : boot_offset + SECTOR_SIZE]
    if boot[82:90] != b"FAT32   ":
        raise ValueError("boot partition is not FAT32")

    bytes_per_sector = struct.unpack_from("<H", boot, 11)[0]
    sectors_per_cluster = boot[13]
    reserved = struct.unpack_from("<H", boot, 14)[0]
    fat_count = boot[16]
    fat_sectors = struct.unpack_from("<I", boot, 36)[0]
    root_cluster = struct.unpack_from("<I", boot, 44)[0]
    if bytes_per_sector != SECTOR_SIZE or sectors_per_cluster == 0:
        raise ValueError("invalid FAT32 geometry")

    cluster_size = bytes_per_sector * sectors_per_cluster
    fat_offset = (partition_start + reserved) * bytes_per_sector
    fat_size = fat_sectors * bytes_per_sector
    fat = bytes(image[fat_offset : fat_offset + fat_size])
    data_offset = (
        partition_start + reserved + fat_count * fat_sectors
    ) * bytes_per_sector

    for directory_cluster in fat_chain(fat, root_cluster):
        directory_offset = data_offset + (directory_cluster - 2) * cluster_size
        directory = image[directory_offset : directory_offset + cluster_size]
        for entry_offset in range(0, len(directory), 32):
            entry = directory[entry_offset : entry_offset + 32]
            if entry[0] in (0x00, 0xE5) or entry[11] == 0x0F:
                continue
            if entry[:11] != short_name:
                continue
            first_cluster = (
                struct.unpack_from("<H", entry, 20)[0] << 16
            ) | struct.unpack_from("<H", entry, 26)[0]
            size = struct.unpack_from("<I", entry, 28)[0]
            return (
                directory_offset + entry_offset,
                data_offset,
                cluster_size,
                size,
                fat_chain(fat, first_cluster),
            )
    raise ValueError(
        f"{short_name.decode('ascii', errors='replace')} not found in FAT32 root directory"
    )


def read_file(image: mmap.mmap, short_name: bytes) -> bytes:
    _, data_offset, cluster_size, size, clusters = locate_file(image, short_name)
    contents = bytearray()
    for cluster in clusters:
        start = data_offset + (cluster - 2) * cluster_size
        contents.extend(image[start : start + cluster_size])
        if len(contents) >= size:
            break
    if len(contents) < size:
        raise ValueError(
            f"{short_name.decode('ascii', errors='replace')} is truncated"
        )
    return bytes(contents[:size])


def replace_file(image: mmap.mmap, short_name: bytes, replacement: bytes) -> None:
    directory_entry, data_offset, cluster_size, old_size, clusters = locate_file(
        image, short_name
    )
    required_clusters = (len(replacement) + cluster_size - 1) // cluster_size
    if required_clusters > len(clusters):
        mbr = image[:SECTOR_SIZE]
        partition_start = struct.unpack_from("<I", mbr, 446 + 8)[0]
        boot_offset = partition_start * SECTOR_SIZE
        boot = image[boot_offset : boot_offset + SECTOR_SIZE]
        reserved = struct.unpack_from("<H", boot, 14)[0]
        fat_count = boot[16]
        fat_sectors = struct.unpack_from("<I", boot, 36)[0]
        fat_offset = (partition_start + reserved) * SECTOR_SIZE
        fat_size = fat_sectors * SECTOR_SIZE
        fat = bytearray(image[fat_offset : fat_offset + fat_size])
        used = set(clusters)
        additional = required_clusters - len(clusters)
        free = []
        for cluster in range(2, len(fat) // 4):
            value = struct.unpack_from("<I", fat, cluster * 4)[0] & 0x0FFFFFFF
            if value == 0 and cluster not in used:
                free.append(cluster)
                if len(free) == additional:
                    break
        if len(free) != additional:
            raise ValueError("FAT32 partition has insufficient free clusters")

        extension = [clusters[-1], *free]
        for index, cluster in enumerate(extension):
            next_cluster = 0x0FFFFFFF if index == len(extension) - 1 else extension[index + 1]
            struct.pack_into("<I", fat, cluster * 4, next_cluster)
        for copy in range(fat_count):
            start = fat_offset + copy * fat_size
            image[start : start + fat_size] = fat
        clusters.extend(free)

    remaining = old_size
    for cluster in clusters:
        length = min(cluster_size, remaining)
        start = data_offset + (cluster - 2) * cluster_size
        image[start : start + length] = b"\x00" * length
        remaining -= length
        if remaining == 0:
            break

    written = 0
    for cluster in clusters:
        length = min(cluster_size, len(replacement) - written)
        if length == 0:
            break
        start = data_offset + (cluster - 2) * cluster_size
        image[start : start + length] = replacement[written : written + length]
        written += length

    struct.pack_into("<I", image, directory_entry + 28, len(replacement))
    if read_file(image, short_name) != replacement:
        raise ValueError(
            f"replacement verification failed for "
            f"{short_name.decode('ascii', errors='replace')}"
        )


def progress_line(phase: str, completed: int, total: int, started: float) -> None:
    elapsed = max(time.monotonic() - started, 0.001)
    rate = completed / elapsed
    remaining = max(total - completed, 0)
    eta = remaining / rate if rate else 0.0
    print(
        f"{phase}_progress bytes={completed}/{total} "
        f"percent={100 * completed / total:.1f} "
        f"rate_mib_s={rate / (1024 * 1024):.1f} eta_s={eta:.1f}",
        flush=True,
    )


def copy_image(source: Path, output: Path) -> None:
    total = source.stat().st_size
    completed = 0
    started = time.monotonic()
    report_at = 0
    print(f"copy_start bytes={total} source={source} output={output}", flush=True)
    with source.open("rb") as input_stream, output.open("wb") as output_stream:
        while chunk := input_stream.read(16 * 1024 * 1024):
            output_stream.write(chunk)
            completed += len(chunk)
            if completed >= report_at or completed == total:
                progress_line("copy", completed, total, started)
                report_at = completed + 256 * 1024 * 1024
    print(f"copy_done elapsed_s={time.monotonic() - started:.1f}", flush=True)


def sha256(path: Path, phase: str) -> str:
    digest = hashlib.sha256()
    total = path.stat().st_size
    completed = 0
    started = time.monotonic()
    report_at = 0
    print(f"{phase}_start bytes={total} path={path}", flush=True)
    with path.open("rb") as stream:
        while chunk := stream.read(16 * 1024 * 1024):
            digest.update(chunk)
            completed += len(chunk)
            if completed >= report_at or completed == total:
                progress_line(phase, completed, total, started)
                report_at = completed + 512 * 1024 * 1024
    result = digest.hexdigest()
    print(
        f"{phase}_done elapsed_s={time.monotonic() - started:.1f} sha256={result}",
        flush=True,
    )
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("kernel", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--config",
        type=Path,
        help="replace CONFIG.TXT with this file after replacing KERNEL7.IMG",
    )
    args = parser.parse_args()

    kernel = args.kernel.read_bytes()
    config = args.config.read_bytes() if args.config else None
    args.output.parent.mkdir(parents=True, exist_ok=True)
    copy_image(args.source, args.output)

    print("fat_replace_start", flush=True)
    with args.output.open("r+b") as stream:
        with mmap.mmap(stream.fileno(), 0) as image:
            replace_file(image, b"KERNEL7 IMG", kernel)
            if config is not None:
                replace_file(image, b"CONFIG  TXT", config)
            image.flush()
    print("fat_replace_done", flush=True)

    print(f"source_sha256={sha256(args.source, 'source_hash')}")
    print(f"kernel_sha256={hashlib.sha256(kernel).hexdigest()}")
    if config is not None:
        print(f"config_sha256={hashlib.sha256(config).hexdigest()}")
    print(f"output_sha256={sha256(args.output, 'output_hash')}")
    print(f"kernel_size={len(kernel)}")
    if config is not None:
        print(f"config_size={len(config)}")
    print(f"output={args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
