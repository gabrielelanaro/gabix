#!/usr/bin/env python3
"""Run a trusted ARM kernel probe through Gabix's authenticated Wi-Fi development build."""
import argparse
import binascii
import importlib.util
import os
import secrets
import socket
import struct
from pathlib import Path

spec = importlib.util.spec_from_file_location("transfer", Path(__file__).with_name("pi2-sd-transfer.py"))
transfer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(transfer)

BEGIN, WRITE, RUN, READ_RESULT = range(20, 24)


def load(connection, image):
    if not 4 <= len(image) <= 256 * 1024:
        raise ValueError("Probe must fit the 256 KiB code region")
    session = secrets.randbelow(0xffffffff) + 1
    transfer.exchange(connection, session, BEGIN, len(image), binascii.crc32(image))
    for offset in range(0, len(image), 512):
        transfer.exchange(connection, session, WRITE, offset, payload=image[offset:offset + 512])
    return session


def run(connection, session, data):
    if len(data) > 512:
        raise ValueError("Input exceeds 512 bytes")
    reply = transfer.exchange(connection, session, RUN, payload=data)
    count, = struct.unpack("<i", reply)
    if not 0 <= count <= 32768:
        raise RuntimeError(f"Probe returned {count}")
    result = bytearray()
    while len(result) < count:
        chunk = transfer.exchange(connection, session, READ_RESULT, len(result))
        if len(chunk) != min(512, count - len(result)):
            raise RuntimeError("Unexpected probe result chunk length")
        result.extend(chunk)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--image", type=Path, help="Upload a new flat image, then run it")
    source.add_argument("--session", type=int, help="Run a previously loaded image again")
    parser.add_argument("--address", default="gabix-pi2.local")
    parser.add_argument("--input-hex", default="", type=bytes.fromhex)
    parser.add_argument("--output", type=Path, help="File for binary result bytes")
    args = parser.parse_args()
    if os.environ.get("GABIX_PI_LEGACY") == "1":
        parser.error("RAM probes require authenticated management")
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.connect((args.address, 5556))
        session = load(connection, args.image.read_bytes()) if args.image else args.session
        print(f"Session {session}", flush=True)
        result = run(connection, session, args.input_hex)
    if args.output:
        args.output.write_bytes(result)
        print(f"Received {len(result)} bytes")
    else:
        print(result.hex())


if __name__ == "__main__":
    main()
