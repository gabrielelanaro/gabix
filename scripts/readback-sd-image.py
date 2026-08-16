#!/usr/bin/env python3

import argparse
import hashlib
from datetime import datetime
import subprocess
import time


def log_factory(path: str):
    def log(message: str) -> None:
        line = f"{datetime.now().astimezone().isoformat(timespec='seconds')} {message}"
        print(line, flush=True)
        with open(path, "a", buffering=1) as logfile:
            logfile.write(line + "\n")

    return log


def find_device(label: str) -> str:
    try:
        listing = subprocess.run(
            ["diskutil", "list", "external", "physical"],
            capture_output=True,
            text=True,
            check=True,
            timeout=10,
        ).stdout
    except subprocess.TimeoutExpired as error:
        raise OSError("diskutil list timed out") from error
    device = None
    for line in listing.splitlines():
        if line.startswith("/dev/disk"):
            device = line.split()[0]
        elif device is not None and label in line:
            return device.replace("/dev/disk", "/dev/rdisk", 1)
    raise OSError(f"could not find external disk with label {label!r}")


def unmount_device(raw_device: str) -> None:
    disk_device = raw_device.replace("/dev/rdisk", "/dev/disk", 1)
    try:
        subprocess.run(
            ["diskutil", "unmountDisk", "force", disk_device],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=10,
        )
    except subprocess.TimeoutExpired as error:
        raise OSError(f"unmount timed out for {disk_device}") from error


def read_block(label: str, block: int, block_size: int) -> tuple[str, bytes]:
    device = find_device(label)
    unmount_device(device)
    try:
        result = subprocess.run(
            [
                "dd",
                f"if={device}",
                f"bs={block_size}",
                f"skip={block}",
                "count=1",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=20,
        )
    except subprocess.TimeoutExpired as error:
        raise OSError(f"{device} read timed out") from error
    if result.returncode != 0 or len(result.stdout) != block_size:
        error = result.stderr.decode(errors="replace").strip()
        raise OSError(
            f"{device} read failed: status={result.returncode}, "
            f"bytes={len(result.stdout)}/{block_size}, {error}"
        )
    return device, result.stdout


def verify(
    label: str,
    image: str,
    block_count: int,
    block_size: int,
    log_path: str,
    max_backoff: float,
    start_block: int,
) -> None:
    log = log_factory(log_path)
    with open(image, "rb") as source_file:
        source = source_file.read()
    expected_size = block_count * block_size
    if len(source) != expected_size:
        raise SystemExit(f"source size {len(source)} != expected {expected_size}")
    source_hash = hashlib.sha256(source).hexdigest()
    read_hash = hashlib.sha256()
    log(
        f"START label={label!r} image={image} blocks={block_count} "
        f"block_size={block_size} start_block={start_block} "
        f"expected_sha256={source_hash}"
    )

    block = start_block
    backoff = 1.0
    read_hash.update(source[: start_block * block_size])
    while block < block_count:
        try:
            device, data = read_block(label, block, block_size)
            expected = source[block * block_size : (block + 1) * block_size]
            if data != expected:
                raise OSError(f"data mismatch at block {block} from {device}")
            read_hash.update(data)
            log(f"SUCCESS block={block} device={device}")
            block += 1
            backoff = 1.0
            if (block - start_block) % 64 == 0 or block == block_count:
                log(
                    f"progress block {block}/{block_count} "
                    f"({100 * block / block_count:.1f}%)"
                )
        except OSError as error:
            log(f"RETRY block={block} backoff={backoff:.1f}s error={error}")
            time.sleep(backoff)
            backoff = min(backoff * 2, max_backoff)

    actual_hash = read_hash.hexdigest()
    if actual_hash != source_hash:
        raise SystemExit(
            f"READBACK_STATUS:1 expected={source_hash} actual={actual_hash}"
        )
    log(f"READBACK_STATUS:0 sha256={actual_hash}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("label")
    parser.add_argument("image")
    parser.add_argument("block_count", type=int)
    parser.add_argument("--block-size", type=int, default=65536)
    parser.add_argument("--log", default="/tmp/gabix-sd-read.log")
    parser.add_argument("--max-backoff", type=float, default=60.0)
    parser.add_argument("--start-block", type=int, default=0)
    arguments = parser.parse_args()
    verify(
        arguments.label,
        arguments.image,
        arguments.block_count,
        arguments.block_size,
        arguments.log,
        arguments.max_backoff,
        arguments.start_block,
    )
