#!/usr/bin/env python3
"""Verify the firmware-facing structure of a Raspberry Pi 2 loader image."""

import argparse
import importlib.util
import mmap
from pathlib import Path
import struct


SECTOR_SIZE = 512


def load_fat_helpers(repo_root: Path):
    helper_path = repo_root / "scripts" / "make-pi2-uart-smoke-from-official.py"
    spec = importlib.util.spec_from_file_location("pi2_fat", helper_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify_fat_metadata(image: mmap.mmap) -> None:
    partition_start = struct.unpack_from("<I", image, 446 + 8)[0]
    boot_offset = partition_start * SECTOR_SIZE
    boot = image[boot_offset : boot_offset + SECTOR_SIZE]
    sectors_per_cluster = boot[13]
    reserved = struct.unpack_from("<H", boot, 14)[0]
    fat_count = boot[16]
    total_sectors = struct.unpack_from("<I", boot, 32)[0]
    fat_sectors = struct.unpack_from("<I", boot, 36)[0]
    fsinfo_sector = struct.unpack_from("<H", boot, 48)[0]
    backup_boot_sector = struct.unpack_from("<H", boot, 50)[0]
    fat_offset = (partition_start + reserved) * SECTOR_SIZE
    fat_size = fat_sectors * SECTOR_SIZE
    first_fat = image[fat_offset : fat_offset + fat_size]

    for copy in range(1, fat_count):
        start = fat_offset + copy * fat_size
        if image[start : start + fat_size] != first_fat:
            raise ValueError(f"FAT copy {copy + 1} differs from FAT copy 1")

    data_sectors = total_sectors - reserved - fat_count * fat_sectors
    data_clusters = data_sectors // sectors_per_cluster
    last_cluster = min(data_clusters + 1, len(first_fat) // 4 - 1)
    free_count = sum(
        1
        for cluster in range(2, last_cluster + 1)
        if struct.unpack_from("<I", first_fat, cluster * 4)[0] & 0x0FFFFFFF == 0
    )
    next_free = next(
        (
            cluster
            for cluster in range(2, last_cluster + 1)
            if struct.unpack_from("<I", first_fat, cluster * 4)[0] & 0x0FFFFFFF == 0
        ),
        0xFFFFFFFF,
    )
    for label, relative_sector in (
        ("primary", fsinfo_sector),
        ("backup", backup_boot_sector + fsinfo_sector),
    ):
        fsinfo_offset = (partition_start + relative_sector) * SECTOR_SIZE
        recorded_free = struct.unpack_from("<I", image, fsinfo_offset + 488)[0]
        recorded_next = struct.unpack_from("<I", image, fsinfo_offset + 492)[0]
        if recorded_free != free_count:
            raise ValueError(
                f"{label} FSInfo free count is {recorded_free}, calculated {free_count}"
            )
        if recorded_next != next_free:
            raise ValueError(
                f"{label} FSInfo next free is {recorded_next}, calculated {next_free}"
            )


def verify(
    image_path: Path, kernel_path: Path, config_path: Path, source_path: Path | None
) -> None:
    repo_root = Path(__file__).resolve().parents[1]
    fat = load_fat_helpers(repo_root)
    expected = {
        b"KERNEL7 IMG": kernel_path.read_bytes(),
        b"CONFIG  TXT": config_path.read_bytes(),
    }
    required = (b"BOOTCODEBIN", b"START   ELF", b"FIXUP   DAT")
    source_firmware = {}
    if source_path is not None:
        with source_path.open("rb") as source_stream, mmap.mmap(
            source_stream.fileno(), 0, access=mmap.ACCESS_READ
        ) as source:
            source_firmware = {name: fat.read_file(source, name) for name in required}

    with image_path.open("rb") as stream, mmap.mmap(
        stream.fileno(), 0, access=mmap.ACCESS_READ
    ) as image:
        verify_fat_metadata(image)
        for name in required:
            contents = fat.read_file(image, name)
            if not contents:
                raise ValueError(f"empty required boot file: {name!r}")
            if source_firmware and contents != source_firmware[name]:
                raise ValueError(f"firmware differs from known-good source: {name!r}")
        for name, contents in expected.items():
            if fat.read_file(image, name) != contents:
                raise ValueError(f"boot-file mismatch: {name!r}")

        _, _, _, _, clusters = fat.locate_file(image, b"KERNEL7 IMG")
        if any(right != left + 1 for left, right in zip(clusters, clusters[1:])):
            raise ValueError(f"KERNEL7.IMG is fragmented: {clusters}")

    print(f"loader_image={image_path}")
    print(f"kernel_clusters={clusters[0]}-{clusters[-1]}")
    print("loader_image_status=PASS")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("image", type=Path)
    parser.add_argument("kernel", type=Path)
    parser.add_argument("config", type=Path)
    parser.add_argument("--source", type=Path)
    args = parser.parse_args()
    verify(args.image, args.kernel, args.config, args.source)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
