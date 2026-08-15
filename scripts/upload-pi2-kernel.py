#!/usr/bin/env python3
"""Upload a development kernel to the Gabix Raspberry Pi 2 UART loader."""

import argparse
import binascii
import os
import select
import struct
import subprocess
import sys
import termios
import time
import tty

MARKER = b"GABIXLOAD"
VERSION = 1
MAX_KERNEL_SIZE = 16 * 1024 * 1024
READY = b"GABIX LOADER READY"


def build_upload(kernel: bytes) -> bytes:
    if not kernel or len(kernel) > MAX_KERNEL_SIZE:
        raise ValueError(f"kernel size must be 1..{MAX_KERNEL_SIZE} bytes")
    header = struct.pack("<III", VERSION, len(kernel), binascii.crc32(kernel))
    return MARKER + header + kernel


def validate_transfer_options(chunk_size: int, retries: int) -> None:
    if chunk_size <= 0:
        raise ValueError("chunk size must be positive")
    if retries <= 0:
        raise ValueError("retries must be positive")


def open_uart(path: str) -> int:
    owners = subprocess.run(
        ["lsof", "-t", path], capture_output=True, text=True, check=False
    ).stdout.strip()
    if owners:
        raise OSError(f"serial device is already open by process(es): {owners}")
    fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    tty.setraw(fd)
    attrs = termios.tcgetattr(fd)
    attrs[4] = termios.B115200
    attrs[5] = termios.B115200
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    return fd


def wait_for(fd: int, marker: bytes, timeout: float, echo: bool = True) -> bytes:
    deadline = time.monotonic() + timeout
    received = bytearray()
    while time.monotonic() < deadline:
        readable, _, _ = select.select([fd], [], [], min(0.2, deadline - time.monotonic()))
        if not readable:
            continue
        chunk = os.read(fd, 4096)
        if echo and chunk:
            sys.stdout.buffer.write(chunk)
            sys.stdout.buffer.flush()
        received.extend(chunk)
        if marker in received:
            return bytes(received)
        if len(received) > 8192:
            del received[:-4096]
    raise TimeoutError(f"timed out waiting for {marker.decode(errors='replace')}")


def wait_for_result(fd: int, timeout: float) -> bool:
    deadline = time.monotonic() + timeout
    received = bytearray()
    while time.monotonic() < deadline:
        readable, _, _ = select.select([fd], [], [], min(0.2, deadline - time.monotonic()))
        if not readable:
            continue
        chunk = os.read(fd, 4096)
        sys.stdout.buffer.write(chunk)
        sys.stdout.buffer.flush()
        received.extend(chunk)
        if b"UPLOAD OK" in received:
            return True
        if b"UPLOAD FAILED" in received:
            return False
    raise TimeoutError("timed out waiting for upload result")


def write_all(fd: int, data: bytes, chunk_size: int) -> None:
    started = time.monotonic()
    sent = 0
    total = len(data)
    while sent < total:
        _, writable, _ = select.select([], [fd], [], 1.0)
        if not writable:
            continue
        count = os.write(fd, data[sent : sent + chunk_size])
        sent += count
        elapsed = max(time.monotonic() - started, 0.001)
        rate = sent / elapsed
        eta = (total - sent) / rate
        print(
            f"\rupload {sent}/{total} bytes ({sent * 100 / total:5.1f}%) "
            f"{rate / 1024:6.1f} KiB/s ETA {eta:5.1f}s",
            end="",
            file=sys.stderr,
            flush=True,
        )
    print(file=sys.stderr)
    termios.tcdrain(fd)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("uart", help="serial device, for example /dev/cu.usbserial-0001")
    parser.add_argument("kernel")
    parser.add_argument("--ready-timeout", type=float, default=30.0)
    parser.add_argument("--result-timeout", type=float, default=10.0)
    parser.add_argument("--chunk-size", type=int, default=4096)
    parser.add_argument("--retries", type=int, default=3)
    parser.add_argument("--wait-ready", action="store_true")
    args = parser.parse_args()
    validate_transfer_options(args.chunk_size, args.retries)

    kernel = open(args.kernel, "rb").read()
    upload = build_upload(kernel)
    fd = open_uart(args.uart)
    try:
        if args.wait_ready:
            print(f"waiting for loader on {args.uart}...", file=sys.stderr)
            wait_for(fd, READY, args.ready_timeout)
        for attempt in range(1, args.retries + 1):
            print(
                f"sending {len(kernel)}-byte kernel (attempt {attempt}/{args.retries})",
                file=sys.stderr,
            )
            write_all(fd, upload, args.chunk_size)
            if wait_for_result(fd, args.result_timeout):
                return 0
            print("loader rejected the upload; retrying complete image", file=sys.stderr)
        return 1
    finally:
        os.close(fd)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, TimeoutError, ValueError) as error:
        print(f"upload failed: {error}", file=sys.stderr)
        raise SystemExit(1)
