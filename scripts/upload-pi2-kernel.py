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


def validate_transfer_options(
    chunk_size: int, retries: int, inter_byte_delay_us: int
) -> None:
    if chunk_size <= 0:
        raise ValueError("chunk size must be positive")
    if retries <= 0:
        raise ValueError("retries must be positive")
    if inter_byte_delay_us < 0:
        raise ValueError("inter-byte delay must not be negative")


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


def wait_for_result(
    fd: int,
    timeout: float,
    success_marker: bytes = b"UPLOAD OK",
    failure_marker: bytes = b"UPLOAD FAILED",
) -> tuple[bool, bytes]:
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
        if success_marker in received:
            return True, bytes(received)
        if failure_marker in received:
            return False, bytes(received)
    raise TimeoutError("timed out waiting for upload result")


def wait_for_marker_after(
    fd: int,
    initial: bytes,
    after_marker: bytes,
    marker: bytes,
    timeout: float,
) -> bool:
    received = bytearray(initial.split(after_marker, 1)[-1])
    if marker in received:
        return True
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        readable, _, _ = select.select(
            [fd], [], [], min(0.2, deadline - time.monotonic())
        )
        if not readable:
            continue
        chunk = os.read(fd, 4096)
        if chunk:
            sys.stdout.buffer.write(chunk)
            sys.stdout.buffer.flush()
            received.extend(chunk)
            if marker in received:
                return True
            if len(received) > 8192:
                del received[:-4096]
    return False


def wait_for_reboot(fd: int, initial: bytes, timeout: float) -> bool:
    return wait_for_marker_after(
        fd,
        initial,
        after_marker=b"UPLOAD OK",
        marker=b"GABIX LOADER READY",
        timeout=timeout,
    )


def write_all(
    fd: int, data: bytes, chunk_size: int, inter_byte_delay_us: int = 0
) -> None:
    started = time.monotonic()
    next_progress = started
    sent = 0
    total = len(data)
    paced = inter_byte_delay_us > 0
    write_size = 1 if paced else chunk_size
    delay_seconds = inter_byte_delay_us / 1_000_000
    while sent < total:
        _, writable, _ = select.select([], [fd], [], 1.0)
        if not writable:
            continue
        count = os.write(fd, data[sent : sent + write_size])
        sent += count
        if paced and sent < total:
            time.sleep(delay_seconds)
        now = time.monotonic()
        if sent == total or now >= next_progress:
            elapsed = max(now - started, 0.001)
            rate = sent / elapsed
            eta = (total - sent) / rate
            print(
                f"\rupload {sent}/{total} bytes ({sent * 100 / total:5.1f}%) "
                f"{rate / 1024:6.1f} KiB/s ETA {eta:5.1f}s",
                end="",
                file=sys.stderr,
                flush=True,
            )
            next_progress = now + 0.25
    print(file=sys.stderr)
    # A paced upload writes more slowly than the UART can transmit, so no
    # device-side backlog remains. Some macOS USB-serial drivers can block in
    # tcdrain indefinitely after a large paced transfer.
    if not paced:
        termios.tcdrain(fd)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("uart", help="serial device, for example /dev/cu.usbserial-0001")
    parser.add_argument("kernel")
    parser.add_argument("--ready-timeout", type=float, default=30.0)
    parser.add_argument("--result-timeout", type=float, default=10.0)
    parser.add_argument("--chunk-size", type=int, default=4096)
    parser.add_argument(
        "--inter-byte-delay-us",
        type=int,
        default=0,
        help="microseconds to wait between bytes; 500 is proven for the current Pi 2 debug loader",
    )
    parser.add_argument("--retries", type=int, default=3)
    parser.add_argument("--wait-ready", action="store_true")
    completion = parser.add_mutually_exclusive_group()
    completion.add_argument(
        "--wait-for-reboot",
        action="store_true",
        help="keep the UART open after UPLOAD OK and require a new loader-ready line",
    )
    completion.add_argument(
        "--wait-for-user-ready",
        action="store_true",
        help="keep the UART open after UPLOAD OK and require the kernel user loader",
    )
    parser.add_argument("--reboot-timeout", type=float, default=15.0)
    parser.add_argument("--user-ready-timeout", type=float, default=30.0)
    args = parser.parse_args()
    validate_transfer_options(
        args.chunk_size, args.retries, args.inter_byte_delay_us
    )

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
            write_all(
                fd,
                upload,
                args.chunk_size,
                args.inter_byte_delay_us,
            )
            uploaded, result = wait_for_result(fd, args.result_timeout)
            if uploaded:
                if args.wait_for_reboot and not wait_for_reboot(
                    fd, result, args.reboot_timeout
                ):
                    raise TimeoutError("timed out waiting for loader after reboot")
                if args.wait_for_user_ready and not wait_for_marker_after(
                    fd,
                    result,
                    after_marker=b"UPLOAD OK",
                    marker=b"GABIX USER READY",
                    timeout=args.user_ready_timeout,
                ):
                    raise TimeoutError("timed out waiting for kernel user loader")
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
