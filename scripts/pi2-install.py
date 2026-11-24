#!/usr/bin/env python3
"""Prepare and apply bounded SD installation packages over the Pi's direct link.

prepare reads the live card and writes local packages plus a burnable image.
apply changes only sectors approved by the kernel's installer. Keep the storage
and loader packages separate: test the candidate loader in RAM between them.
"""

import argparse
import binascii
import hashlib
import importlib.util
import json
import mmap
import os
from pathlib import Path
import secrets
import shutil
import socket
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[1]
START = 4882432
COUNT = 262144
NONE = 0xFFFFFFFF
MAX_KERNEL = 16 * 1024 * 1024


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


client = load_module("sd_client", ROOT / "scripts/pi2-sd-transfer.py")
fat = load_module("fat", ROOT / "scripts/make-pi2-uart-smoke-from-official.py")


class Card:
    def __init__(self, address):
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.connect((address, 5556))
        self.session = secrets.randbelow(NONE) + 1
        self.sectors = struct.unpack("<Q", self.request(8))[0]
        self.cache = {}

    def request(self, operation, argument=0, value=0, payload=b"", timeout=2):
        return client.exchange(
            self.socket, self.session, operation, argument, value, payload, timeout
        )

    def read(self, lba):
        if lba not in self.cache:
            self.cache[lba] = self.request(7, lba)
            if len(self.cache[lba]) != 512:
                raise ValueError("Short sector reply")
        return self.cache[lba]

    def close(self):
        self.socket.close()


def boot_record(kernel, active=0, pending=NONE, copies=None):
    if not 0 < len(kernel) <= MAX_KERNEL:
        raise ValueError("Kernel exceeds the current executable-size limit")
    record = bytearray(512)
    record[:8] = b"GABIXBT1"
    struct.pack_into("<II", record, 8, 1, active)
    if copies is None:
        copies = [
            (2048, 65536, len(kernel), binascii.crc32(kernel)),
            (67584, 65536, len(kernel), binascii.crc32(kernel)),
        ]
    for offset, copy in zip((16, 40), copies):
        struct.pack_into("<QQII", record, offset, *copy)
    struct.pack_into("<III", record, 64, pending, 0, NONE)
    struct.pack_into("<I", record, 508, binascii.crc32(record[:508]))
    return bytes(record)


def partition_mbr(mbr):
    if mbr[510:] != b"\x55\xaa":
        raise ValueError("Invalid MBR")
    expected = [(12, 16384, 1048576), (131, 1064960, 3817472)]
    for index, wanted in enumerate(expected):
        entry = mbr[446 + index * 16 : 462 + index * 16]
        if (entry[4], *struct.unpack_from("<II", entry, 8)) != wanted:
            raise ValueError("Existing partitions differ from the agreed layout")
    if any(mbr[494:510]):
        raise ValueError("Fourth partition is occupied")
    third = mbr[478:494]
    if any(third) and (third[4], *struct.unpack_from("<II", third, 8)) != (
        0xDA,
        START,
        COUNT,
    ):
        raise ValueError("Third partition is occupied")
    result = bytearray(mbr)
    result[478:494] = bytes(16)
    result[482] = 0xDA
    struct.pack_into("<II", result, 486, START, COUNT)
    return bytes(result)


def file_location(read):
    boot = read(16384)
    if boot[11:13] != b"\x00\x02" or boot[82:90] != b"FAT32   ":
        raise ValueError("Expected FAT32")
    cluster_sectors = boot[13]
    reserved = struct.unpack_from("<H", boot, 14)[0]
    fat_size = struct.unpack_from("<I", boot, 36)[0]
    fat_start = 16384 + reserved
    data_start = fat_start + boot[16] * fat_size

    def next_cluster(cluster):
        byte = cluster * 4
        return (
            struct.unpack_from("<I", read(fat_start + byte // 512), byte % 512)[0]
            & 0xFFFFFFF
        )

    def sectors(cluster):
        start = data_start + (cluster - 2) * cluster_sectors
        if cluster < 2 or start + cluster_sectors > 16384 + 1048576:
            raise ValueError("Invalid FAT cluster")
        return range(start, start + cluster_sectors)

    root = struct.unpack_from("<I", boot, 44)[0]
    for _ in range(64):
        for lba in sectors(root):
            entries = read(lba)
            for offset in range(0, 512, 32):
                entry = entries[offset : offset + 32]
                if entry[:11] == b"KERNEL7 IMG":
                    cluster = (
                        struct.unpack_from("<H", entry, 20)[0] << 16
                    ) | struct.unpack_from("<H", entry, 26)[0]
                    result = []
                    seen = set()
                    while len(result) < 512 and cluster < 0xFFFFFF8:
                        if cluster in seen:
                            raise ValueError("Cyclic FAT chain")
                        seen.add(cluster)
                        result.extend(sectors(cluster))
                        cluster = next_cluster(cluster)
                    return lba, offset, result[:512]
                if entry[0] == 0:
                    raise ValueError("KERNEL7.IMG not found")
        root = next_cluster(root)
    raise ValueError("Root directory traversal limit")


def add_bytes(patches, read, start, data):
    for offset in range(0, len(data), 512):
        lba = start + offset // 512
        new = data[offset : offset + 512].ljust(512, b"\0")
        old = read(lba)
        if old != new:
            patches.append((lba, old, new))


def loader_patches(read, loader):
    directory, offset, sectors = file_location(read)
    if not loader or len(loader) > min(len(sectors) * 512, 256 * 1024):
        raise ValueError("Loader exceeds its existing FAT allocation")
    patches = []
    for index, position in enumerate(range(0, len(loader), 512)):
        lba = sectors[index]
        old = read(lba)
        new = loader[position : position + 512].ljust(512, b"\0")
        if old != new:
            patches.append((lba, old, new))
    patches.sort()
    old = read(directory)
    new = bytearray(old)
    struct.pack_into("<I", new, offset + 28, len(loader))
    if old != new:
        patches.append((directory, old, bytes(new)))
    return patches


def write_package(path, patches):
    if not 1 <= len(patches) <= 1000:
        raise ValueError("An installation package must contain 1..1000 changed sectors")
    header = bytearray(512)
    header[:8] = b"GBXINST1"
    struct.pack_into("<II", header, 8, len(patches), 1)
    with path.open("xb") as output:
        output.write(header)
        for lba, old, new in patches:
            descriptor = bytearray(512)
            struct.pack_into(
                "<QII", descriptor, 0, lba, binascii.crc32(old), binascii.crc32(new)
            )
            output.write(descriptor)
            output.write(old)
            output.write(new)
    if path.stat().st_size > client.MAX_BYTES:
        raise ValueError("Package exceeds Ethernet staging capacity")


def make_image(source, path, kernel, loader, record):
    if path.exists() or source.resolve() == path.resolve():
        raise ValueError("Output image must be a new file")
    # Clone on APFS where possible; the protected source is always opened read-only.
    result = subprocess.run(["cp", "-c", str(source), str(path)], capture_output=True)
    if result.returncode:
        shutil.copyfile(source, path)
    # A cloned protected baseline can inherit immutable flags; clear only the new copy.
    if hasattr(os, "chflags"):
        os.chflags(path, 0)
    path.chmod(0o600)
    with path.open("r+b") as output:
        output.truncate((START + COUNT) * 512)
        with mmap.mmap(output.fileno(), 0) as image:
            image[:512] = partition_mbr(image[:512])
            image[START * 512 : (START + 1) * 512] = record
            for offset in (2048, 67584):
                at = (START + offset) * 512
                image[at : at + len(kernel)] = kernel

            def read(lba):
                return image[lba * 512 : (lba + 1) * 512]

            for lba, _, new in loader_patches(read, loader):
                image[lba * 512 : (lba + 1) * 512] = new
            if fat.read_file(image, b"KERNEL7 IMG") != loader:
                raise ValueError("Recovery loader verification failed")
            for offset in (2048, 67584):
                at = (START + offset) * 512
                if image[at : at + len(kernel)] != kernel:
                    raise ValueError("Recovery kernel verification failed")
            image.flush()
    with path.open("rb") as saved:
        digest = hashlib.file_digest(saved, "sha256").hexdigest()
    path.with_suffix(".json").write_text(
        json.dumps(
            {
                "status": "candidate-not-yet-hardware-verified",
                "bytes": path.stat().st_size,
                "sha256": digest,
                "kernel_crc32": f"{binascii.crc32(kernel):08x}",
            },
            indent=2,
        )
        + "\n"
    )


def prepare(args):
    args.output.mkdir(parents=True, exist_ok=True)
    kernel = args.kernel.read_bytes()
    loader = args.loader.read_bytes()
    record = boot_record(kernel)
    # Produce the recovery image before preparing any live-card installation.
    image = args.output / "gabix-pi2-sd-recovery.img"
    make_image(args.source, image, kernel, loader, record)
    card = Card(args.address)
    try:
        if card.sectors < START + COUNT:
            raise ValueError("Card too small")
        mbr = card.read(0)
        new_mbr = partition_mbr(mbr)
        storage = []
        for offset in (2048, 67584):
            add_bytes(storage, card.read, START + offset, kernel)
        storage.sort()
        storage.append((START, card.read(START), record))
        if mbr != new_mbr:
            storage.append((0, mbr, new_mbr))
        write_package(args.output / "storage.pkg", storage)
        write_package(args.output / "loader.pkg", loader_patches(card.read, loader))
        (args.output / "original-mbr.bin").write_bytes(mbr)
        print(f"Prepared storage.pkg, loader.pkg and {image}; no card writes performed")
    finally:
        card.close()


def apply(args):
    data = args.package.read_bytes()
    if data[:8] != b"GBXINST1":
        raise ValueError("Invalid installation package")
    client.upload(args.package, args.address)
    card = Card(args.address)
    try:
        # OPEN binds this connection to the verified staging file.
        if card.request(5) != struct.pack("<II", len(data), binascii.crc32(data)):
            raise ValueError("Staging file differs")
        card.request(6, timeout=60)
        count = struct.unpack_from("<I", data, 8)[0]
        for index in range(count):
            at = (1 + 3 * index) * 512
            lba = struct.unpack_from("<Q", data, at)[0]
            if card.read(lba) != data[at + 1024 : at + 1536]:
                raise ValueError(f"Installed sector {lba} differs")
        print(f"PASS: {count} installed sectors read back exactly")
    finally:
        card.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--address", default="10.0.2.15")
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("prepare")
    p.add_argument("--source", type=Path, required=True)
    p.add_argument("--kernel", type=Path, required=True)
    p.add_argument("--loader", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.set_defaults(run=prepare)
    p = sub.add_parser("apply")
    p.add_argument("package", type=Path)
    p.set_defaults(run=apply)
    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
