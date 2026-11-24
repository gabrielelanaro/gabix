#!/usr/bin/env python3
"""Upload ARM user executables into RAM and report each exit status over Wi-Fi."""
import argparse
import binascii
import importlib.util
import json
import os
from pathlib import Path
import secrets
import struct
import time

spec = importlib.util.spec_from_file_location("management", Path(__file__).with_name("pi2-manage.py"))
management = importlib.util.module_from_spec(spec)
spec.loader.exec_module(management)

BEGIN, WRITE, START, RESULT = range(24, 28)
MAX_IMAGE = 512 * 1024


def start(pi, path):
    image = path.read_bytes()
    if not image.startswith(b"\x7fELF") or len(image) > MAX_IMAGE:
        raise ValueError("Expected an ARM ELF executable of at most 512 KiB")
    session = secrets.randbelow(0xffffffff) + 1
    exchange = management.client.exchange
    exchange(pi.connection, session, BEGIN, len(image), binascii.crc32(image))
    for offset in range(0, len(image), 512):
        exchange(pi.connection, session, WRITE, offset, payload=image[offset:offset + 512])
    exchange(pi.connection, session, START)
    return session


def wait_for_exit(pi, session, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = management.client.exchange(pi.connection, session, RESULT)
        if result:
            return struct.unpack("<I", result)[0]
        time.sleep(0.05)
    raise TimeoutError(f"No exit status from user session {session}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("elf", type=Path, nargs="+")
    parser.add_argument("--address", default="gabix-pi2.local")
    parser.add_argument("--wait-seconds", type=float, default=15, help="Time to wait for user exit")
    args = parser.parse_args()
    if os.environ.get("GABIX_PI_LEGACY") == "1":
        parser.error("Wi-Fi user upload requires authenticated management")
    pi = management.Pi(args.address)
    try:
        initial = pi.status()
        for path in args.elf:
            code = wait_for_exit(pi, start(pi, path), args.wait_seconds)
            status = pi.status()
            if (status["boot"], status["kernel_crc32"]) != (initial["boot"], initial["kernel_crc32"]):
                raise RuntimeError("Pi boot or installed kernel changed during the run")
            print(json.dumps({"elf": path.name, "boot": status["boot"],
                              "kernel_crc32": status["kernel_crc32"], "exit": code}), flush=True)
    finally:
        pi.connection.close()


if __name__ == "__main__":
    main()
