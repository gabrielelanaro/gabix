#!/usr/bin/env python3

import argparse
from datetime import datetime
import subprocess
import time


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


def write_image(
    label: str,
    image: str,
    block_count: int,
    block_size: int,
    log_path: str,
    max_backoff: float,
    start_block: int,
) -> None:
    def log(message: str) -> None:
        line = f"{datetime.now().astimezone().isoformat(timespec='seconds')} {message}"
        print(line, flush=True)
        with open(log_path, "a", buffering=1) as logfile:
            logfile.write(line + "\n")

    block = start_block
    end_block = start_block + block_count
    backoff = 1.0
    log(
        f"START label={label!r} image={image} blocks={block_count} "
        f"block_size={block_size} max_backoff={max_backoff} "
        f"start_block={start_block}"
    )
    while block < end_block:
        device = None
        try:
            device = find_device(label)
            log(f"ATTEMPT block={block} device={device}")
            unmount_device(device)
            result = subprocess.run(
                [
                    "dd",
                    f"if={image}",
                    f"of={device}",
                    f"bs={block_size}",
                    f"skip={block}",
                    f"seek={block}",
                    "count=1",
                    "conv=sync",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=20,
            )
            if result.returncode != 0:
                error = result.stderr.decode(errors="replace").strip()
                raise OSError(f"{device} write failed: {error}")
            block += 1
            log(f"SUCCESS block={block - 1} device={device}")
            backoff = 1.0
            if (block - start_block) % 64 == 0 or block == end_block:
                log(
                    f"progress block {block}/{end_block} "
                    f"({100 * (block - start_block) / block_count:.1f}%)",
                )
        except (OSError, subprocess.TimeoutExpired) as error:
            log(f"RETRY block={block} backoff={backoff:.1f}s error={error}")
            time.sleep(backoff)
            backoff = min(backoff * 2, max_backoff)

    log("SYNC start")
    subprocess.run(["sync"], check=True)
    log("WRITE_STATUS:0")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("label")
    parser.add_argument("image")
    parser.add_argument("block_count", type=int)
    parser.add_argument("--block-size", type=int, default=65536)
    parser.add_argument("--log", default="/tmp/gabix-sd-write.log")
    parser.add_argument("--max-backoff", type=float, default=60.0)
    parser.add_argument("--start-block", type=int, default=0)
    arguments = parser.parse_args()
    write_image(
        arguments.label,
        arguments.image,
        arguments.block_count,
        arguments.block_size,
        arguments.log,
        arguments.max_backoff,
        arguments.start_block,
    )
