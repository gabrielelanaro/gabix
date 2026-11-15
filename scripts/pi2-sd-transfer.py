#!/usr/bin/env python3
"""Upload a file to the Pi's reserved SD region, then read it back byte for byte.

For the opt-in pi2-sd-transfer kernel on the private direct Ethernet link.
Default uploads store a staging file. --kernel writes the inactive kernel copy
and marks it for a trial boot after whole-image verification.
"""

import argparse
import binascii
import hashlib
import hmac
import os
import weakref
from pathlib import Path
import secrets
import socket
import struct
import time

HEADER = struct.Struct("<4sIIII")
MAGIC = b"GBSD"
MAX_BYTES = 4095 * 512


def _exchange_plain(
    connection, session, operation, argument=0, value=0, payload=b"", timeout=2
):
    packet = HEADER.pack(MAGIC, operation, session, argument, value) + payload
    for _ in range(8):
        connection.send(packet)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            connection.settimeout(max(0.001, deadline - time.monotonic()))
            try:
                reply = connection.recv(1024)
            except socket.timeout:
                break
            if len(reply) < HEADER.size:
                continue
            magic, reply_op, reply_session, reply_argument, status = HEADER.unpack_from(
                reply
            )
            if (magic, reply_op, reply_session, reply_argument) != (
                MAGIC,
                operation | 0x80000000,
                session,
                argument,
            ):
                continue
            if status:
                raise RuntimeError(
                    f"Pi rejected operation {operation}, offset {argument}: status {status}"
                )
            return reply[HEADER.size :]
    raise TimeoutError(
        f"No acknowledgement for operation {operation}, offset {argument}"
    )


_AUTH = weakref.WeakKeyDictionary()


def exchange(
    connection, session, operation, argument=0, value=0, payload=b"", timeout=2
):
    if os.environ.get("GABIX_PI_LEGACY") == "1":
        return _exchange_plain(
            connection, session, operation, argument, value, payload, timeout
        )
    state = _AUTH.get(connection)
    if state is None:
        path = Path(
            os.environ.get(
                "GABIX_PI_MANAGEMENT_KEY",
                Path.home() / ".config/gabix/pi2-management.key",
            )
        )
        key = path.read_bytes()
        if len(key) != 32:
            raise ValueError("Management key must contain exactly 32 bytes")
        boot, sequence = struct.unpack("<QQ", _exchange_plain(connection, session, 10))
        state = [key, boot, sequence]
        _AUTH[connection] = state
    key, boot, sequence = state
    sequence += 1
    inner = HEADER.pack(MAGIC, operation, session, argument, value) + payload
    signed = struct.pack("<4sQQ", b"GBAU", boot, sequence) + inner
    packet = signed + hmac.digest(key, b"request" + signed, "sha256")
    for _ in range(8):
        connection.send(packet)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            connection.settimeout(max(0.001, deadline - time.monotonic()))
            try:
                reply = connection.recv(1024)
            except socket.timeout:
                break
            if len(reply) < 72 or reply[:20] != struct.pack(
                "<4sQQ", b"GBAU", boot, sequence
            ):
                continue
            if not hmac.compare_digest(
                reply[-32:], hmac.digest(key, b"response" + reply[:-32], "sha256")
            ):
                continue
            response = reply[20:-32]
            magic, reply_op, reply_session, reply_argument, status = HEADER.unpack_from(
                response
            )
            if (magic, reply_op, reply_session, reply_argument) != (
                MAGIC,
                operation | 0x80000000,
                session,
                argument,
            ):
                continue
            state[2] = sequence
            if status:
                raise RuntimeError(
                    f"Pi rejected operation {operation}, offset {argument}: status {status}"
                )
            return response[HEADER.size :]
    # Another authorized client may have advanced the counter; the next command
    # obtains a new challenge. Never fall back to an unsigned request.
    _AUTH.pop(connection, None)
    raise TimeoutError(
        f"No authenticated acknowledgement for operation {operation}; check address/key/link"
    )


def upload(path, address, verify_only=False, kernel=False):
    data = path.read_bytes()
    limit = 16 * 1024 * 1024 if kernel else MAX_BYTES
    if not 0 < len(data) <= limit:
        raise ValueError(f"File must contain 1..{limit} bytes")
    checksum = binascii.crc32(data)
    session = secrets.randbelow(0xFFFFFFFF) + 1
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.connect((address, 5556))
        started = time.monotonic()
        if verify_only:
            metadata = exchange(connection, session, 5)
            if metadata != struct.pack("<II", len(data), checksum):
                raise RuntimeError("Stored file metadata differs from the local file")
        else:
            exchange(connection, session, 9 if kernel else 1, len(data), checksum)
            for offset in range(0, len(data), 512):
                chunk = data[offset : offset + 512]
                exchange(connection, session, 2, offset, binascii.crc32(chunk), chunk)
            exchange(connection, session, 3)
        for offset in range(0, len(data), 512):
            received = exchange(connection, session, 4, offset)
            if received != data[offset : offset + 512]:
                raise RuntimeError(f"SD read-back differs at offset {offset}")
        print(
            f"PASS: {len(data)} bytes stored on SD and read back exactly; CRC32 {checksum:08x}; {time.monotonic()-started:.2f}s"
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=Path)
    parser.add_argument("--address", default="10.0.2.15")
    parser.add_argument(
        "--verify-only",
        action="store_true",
        help="Read the stored file without writing; also works after a reboot",
    )
    parser.add_argument(
        "--kernel",
        action="store_true",
        help="Write the inactive kernel copy and mark it for trial boot",
    )
    args = parser.parse_args()
    if args.kernel and args.verify_only:
        parser.error("--verify-only applies to the staging file")
    upload(args.file, args.address, args.verify_only, args.kernel)


if __name__ == "__main__":
    main()
