#!/usr/bin/env python3

import argparse
import hashlib
from datetime import datetime
import subprocess
import time


def make_log(path: str):
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


def unmount(device: str) -> None:
    disk = device.replace("/dev/rdisk", "/dev/disk", 1)
    try:
        subprocess.run(
            ["diskutil", "unmountDisk", "force", disk],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=10,
        )
    except subprocess.TimeoutExpired as error:
        raise OSError(f"unmount timed out for {disk}") from error


def run_dd(command: list[str], block_size: int, timeout: float = 20) -> bytes:
    try:
        result = subprocess.run(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as error:
        raise OSError("dd timed out") from error
    if result.returncode != 0:
        error = result.stderr.decode(errors="replace").strip()
        raise OSError(f"dd failed: {error}")
    if len(result.stdout) not in (0, block_size):
        raise OSError(f"short dd output: {len(result.stdout)} bytes")
    return result.stdout


def repair(
    label: str,
    image: str,
    block: int,
    block_size: int,
    log_path: str,
    max_attempts: int,
    max_backoff: float,
) -> None:
    log = make_log(log_path)
    with open(image, "rb") as source_file:
        source_file.seek(block * block_size)
        expected = source_file.read(block_size)
    expected_hash = hashlib.sha256(expected).hexdigest()
    log(
        f"START label={label!r} image={image} block={block} "
        f"expected_sha256={expected_hash} max_attempts={max_attempts}"
    )

    backoff = 1.0
    for attempt in range(1, max_attempts + 1):
        try:
            device = find_device(label)
            unmount(device)
            run_dd(
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
                block_size,
            )
            device = find_device(label)
            unmount(device)
            actual = run_dd(
                [
                    "dd",
                    f"if={device}",
                    f"bs={block_size}",
                    f"skip={block}",
                    "count=1",
                ],
                block_size,
            )
            actual_hash = hashlib.sha256(actual).hexdigest()
            if actual == expected:
                log(f"SUCCESS attempt={attempt} block={block} sha256={actual_hash}")
                return
            raise OSError(
                f"mismatch expected={expected_hash} actual={actual_hash}"
            )
        except OSError as error:
            log(
                f"RETRY attempt={attempt} block={block} "
                f"backoff={backoff:.1f}s error={error}"
            )
            time.sleep(backoff)
            backoff = min(backoff * 2, max_backoff)

    raise SystemExit(f"FAILED block={block} after {max_attempts} attempts")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("label")
    parser.add_argument("image")
    parser.add_argument("block", type=int)
    parser.add_argument("--block-size", type=int, default=65536)
    parser.add_argument("--log", default="/tmp/gabix-block-repair.log")
    parser.add_argument("--max-attempts", type=int, default=20)
    parser.add_argument("--max-backoff", type=float, default=60.0)
    arguments = parser.parse_args()
    repair(
        arguments.label,
        arguments.image,
        arguments.block,
        arguments.block_size,
        arguments.log,
        arguments.max_attempts,
        arguments.max_backoff,
    )
