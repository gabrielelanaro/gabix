#!/usr/bin/env python3
"""Write an image with increasing block sizes and verify its FAT32 boot files."""

from __future__ import annotations

import argparse
from datetime import datetime
import hashlib
from pathlib import Path
import struct
import subprocess
import sys
import time


SECTOR_SIZE = 512
DEFAULT_SIZES = (
    65536,
    131072,
    262144,
    524288,
    1048576,
    2097152,
    4194304,
    8388608,
    16777216,
)
BOOT_FILES = (b"BOOTCODEBIN", b"START   ELF", b"FIXUP   DAT", b"KERNEL7 IMG", b"CONFIG  TXT")


def log(path: Path, message: str) -> None:
    line = f"{datetime.now().astimezone().isoformat(timespec='seconds')} {message}"
    print(line, flush=True)
    with path.open("a", buffering=1) as stream:
        stream.write(line + "\n")


def unmount(raw_device: str) -> None:
    disk_device = raw_device.replace("/dev/rdisk", "/dev/disk", 1)
    subprocess.run(
        ["diskutil", "unmountDisk", "force", disk_device],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=15,
    )


def fat32_files(image: bytes) -> dict[bytes, bytes]:
    if image[510:512] != b"\x55\xaa":
        raise ValueError("missing MBR signature")
    partition = image[446:462]
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
    data_offset = (partition_start + reserved + fat_count * fat_sectors) * bytes_per_sector
    fat_offset = (partition_start + reserved) * bytes_per_sector
    fat = image[fat_offset : fat_offset + fat_sectors * bytes_per_sector]
    root = bytearray()
    cluster = root_cluster
    seen_root: set[int] = set()
    while True:
        if cluster < 2 or cluster in seen_root:
            raise ValueError("broken FAT root directory chain")
        seen_root.add(cluster)
        root_offset = data_offset + (cluster - 2) * cluster_size
        root.extend(image[root_offset : root_offset + cluster_size])
        next_cluster = struct.unpack_from("<I", fat, cluster * 4)[0] & 0x0FFFFFFF
        if next_cluster >= 0x0FFFFFF8:
            break
        cluster = next_cluster

    files: dict[bytes, bytes] = {}
    for offset in range(0, len(root), 32):
        entry = root[offset : offset + 32]
        if entry[0] in (0, 0xE5) or entry[11] == 0x0F:
            continue
        name = bytes(entry[:11])
        first_cluster = (struct.unpack_from("<H", entry, 20)[0] << 16) | struct.unpack_from(
            "<H", entry, 26
        )[0]
        size = struct.unpack_from("<I", entry, 28)[0]
        data = bytearray()
        seen: set[int] = set()
        cluster = first_cluster
        while len(data) < size:
            if cluster < 2 or cluster in seen:
                raise ValueError(f"broken FAT chain for {name!r}")
            seen.add(cluster)
            start = data_offset + (cluster - 2) * cluster_size
            data.extend(image[start : start + cluster_size])
            next_cluster = struct.unpack_from("<I", fat, cluster * 4)[0] & 0x0FFFFFFF
            if next_cluster >= 0x0FFFFFF8:
                break
            cluster = next_cluster
        files[name] = bytes(data[:size])
    return files


def verify_boot(source: bytes, raw_device: str) -> tuple[bool, str]:
    expected = fat32_files(source)
    unmount(raw_device)
    with open(raw_device, "rb", buffering=0) as device:
        actual_image = device.read(len(source))
    try:
        actual = fat32_files(actual_image)
        for name in BOOT_FILES:
            if actual.get(name) != expected.get(name):
                expected_hash = hashlib.sha256(expected.get(name, b"")).hexdigest()
                actual_hash = hashlib.sha256(actual.get(name, b"")).hexdigest()
                return False, f"{name.decode('ascii')} expected={expected_hash} actual={actual_hash}"
    except (KeyError, ValueError, struct.error) as error:
        return False, str(error)
    return True, "boot artifacts match"


def write_block(image: str, raw_device: str, block: int, block_size: int) -> None:
    result = subprocess.run(
        [
            "dd",
            f"if={image}",
            f"of={raw_device}",
            f"bs={block_size}",
            f"skip={block}",
            f"seek={block}",
            "count=1",
            "conv=sync",
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
    )
    if result.returncode:
        raise OSError(result.stderr.decode(errors="replace").strip())


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("raw_device")
    parser.add_argument("image")
    parser.add_argument("--log", default="/tmp/gabix-sd-powers.log")
    parser.add_argument("--attempts", type=int, default=3)
    parser.add_argument("--max-backoff", type=float, default=60.0)
    parser.add_argument("--min-size", type=int, default=DEFAULT_SIZES[0])
    arguments = parser.parse_args()

    raw_device = arguments.raw_device
    image_path = Path(arguments.image)
    log_path = Path(arguments.log)
    source = image_path.read_bytes()
    if len(source) % DEFAULT_SIZES[-1] != 0:
        raise SystemExit("image size must be divisible by 4 MiB")
    if arguments.attempts < 1:
        raise SystemExit("--attempts must be positive")

    log(log_path, f"START device={raw_device} image={image_path} bytes={len(source)}")
    for block_size in DEFAULT_SIZES:
        if block_size < arguments.min_size:
            continue
        block_count = len(source) // block_size
        log(log_path, f"SIZE_START block_size={block_size} blocks={block_count}")
        unmount(raw_device)
        block = 0
        while block < block_count:
            backoff = 1.0
            success = False
            for attempt in range(1, arguments.attempts + 1):
                try:
                    log(log_path, f"ATTEMPT size={block_size} block={block} try={attempt}")
                    unmount(raw_device)
                    write_block(str(image_path), raw_device, block, block_size)
                    log(log_path, f"SUCCESS size={block_size} block={block}")
                    success = True
                    break
                except (OSError, subprocess.TimeoutExpired) as error:
                    log(log_path, f"RETRY size={block_size} block={block} try={attempt} backoff={backoff:.1f}s error={error}")
                    if attempt < arguments.attempts:
                        time.sleep(backoff)
                        backoff = min(backoff * 2, arguments.max_backoff)
            if not success:
                log(log_path, f"SIZE_FAILED block_size={block_size} block={block}")
                break
            block += 1
        if block != block_count:
            continue
        subprocess.run(["sync"], check=True)
        ok, detail = verify_boot(source, raw_device)
        log(log_path, f"VERIFY size={block_size} result={'PASS' if ok else 'FAIL'} detail={detail}")
        if ok:
            log(log_path, f"POWERS_STATUS:0 block_size={block_size}")
            return 0
    log(log_path, "POWERS_STATUS:1 no block size passed")
    return 1


if __name__ == "__main__":
    sys.exit(main())
