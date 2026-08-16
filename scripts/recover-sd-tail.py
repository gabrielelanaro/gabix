#!/usr/bin/env python3

import argparse
import os
import subprocess
import time


def find_device(label: str) -> str:
    listing = subprocess.check_output(
        ["diskutil", "list", "external", "physical"],
        text=True,
    )
    device = None
    for line in listing.splitlines():
        if line.startswith("/dev/disk"):
            device = line.split()[0]
        elif device is not None and label in line:
            return device.replace("/dev/disk", "/dev/rdisk", 1)
    raise OSError(f"could not find external disk with label {label!r}")


def read_chunk(
    label: str,
    first_block: int,
    block_count: int,
    block_size: int,
) -> bytes:
    device = find_device(label)
    command = [
        "dd",
        f"if={device}",
        f"bs={block_size}",
        f"skip={first_block}",
        f"count={block_count}",
    ]
    try:
        result = subprocess.run(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=20,
        )
    except subprocess.TimeoutExpired as error:
        raise OSError(f"{device} read timed out at block {first_block}") from error
    expected = block_count * block_size
    if result.returncode != 0 or len(result.stdout) != expected:
        error = result.stderr.decode(errors="replace").strip()
        raise OSError(
            f"{device} read failed at block {first_block}: "
            f"status={result.returncode}, bytes={len(result.stdout)}/{expected}, {error}"
        )
    return result.stdout


def recover(label: str, output: str, end_block: int, block_size: int) -> None:
    size = os.path.getsize(output)
    if size % block_size:
        raise SystemExit(f"output is not aligned to {block_size} bytes: {size}")

    block = size // block_size
    print(f"resume block {block}/{end_block}", flush=True)
    while block < end_block:
        try:
            block_count = min(64, end_block - block)
            data = read_chunk(label, block, block_count, block_size)
            with open(output, "ab", buffering=0) as destination:
                destination.write(data)
            block += block_count
            if block % 1024 == 0 or block == end_block:
                percent = 100 * block / end_block
                print(
                    f"progress block {block}/{end_block} ({percent:.1f}%)",
                    flush=True,
                )
        except OSError as error:
            print(f"retry at block {block}: {error}", flush=True)
            time.sleep(2)

    print(f"RECOVERY_STATUS:0 size={os.path.getsize(output)}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("label")
    parser.add_argument("output")
    parser.add_argument("end_block", type=int)
    parser.add_argument("--block-size", type=int, default=65536)
    arguments = parser.parse_args()
    recover(
        arguments.label,
        arguments.output,
        arguments.end_block,
        arguments.block_size,
    )
