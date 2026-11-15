#!/usr/bin/env python3
"""Upload a file to the Pi's reserved SD region, then read it back byte for byte.

For the opt-in pi2-sd-transfer kernel on the private direct Ethernet link.
This stores a file; it does not change boot selection.
"""
import argparse
import binascii
from pathlib import Path
import secrets
import socket
import struct
import time

HEADER = struct.Struct('<4sIIII')
MAGIC = b'GBSD'
MAX_BYTES = 4095 * 512


def exchange(connection, session, operation, argument=0, value=0, payload=b''):
    packet = HEADER.pack(MAGIC, operation, session, argument, value) + payload
    for _ in range(8):
        connection.send(packet)
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            connection.settimeout(max(0.001, deadline-time.monotonic()))
            try:
                reply = connection.recv(1024)
            except socket.timeout:
                break
            if len(reply) < HEADER.size:
                continue
            magic, reply_op, reply_session, reply_argument, status = HEADER.unpack_from(reply)
            if (magic, reply_op, reply_session, reply_argument) != (MAGIC, operation | 0x80000000, session, argument):
                continue
            if status:
                raise RuntimeError(f'Pi rejected operation {operation}, offset {argument}: status {status}')
            return reply[HEADER.size:]
    raise TimeoutError(f'No acknowledgement for operation {operation}, offset {argument}')


def upload(path, address, verify_only=False):
    data = path.read_bytes()
    if not 0 < len(data) <= MAX_BYTES:
        raise ValueError(f'File must contain 1..{MAX_BYTES} bytes')
    checksum = binascii.crc32(data)
    session = secrets.randbelow(0xffffffff) + 1
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.connect((address, 5556))
        started = time.monotonic()
        if verify_only:
            metadata = exchange(connection, session, 5)
            if metadata != struct.pack('<II', len(data), checksum):
                raise RuntimeError('Stored file metadata differs from the local file')
        else:
            exchange(connection, session, 1, len(data), checksum)
            for offset in range(0, len(data), 512):
                chunk = data[offset:offset+512]
                exchange(connection, session, 2, offset, binascii.crc32(chunk), chunk)
            exchange(connection, session, 3)
        for offset in range(0, len(data), 512):
            received = exchange(connection, session, 4, offset)
            if received != data[offset:offset+512]:
                raise RuntimeError(f'SD read-back differs at offset {offset}')
        print(f'PASS: {len(data)} bytes stored on SD and read back exactly; CRC32 {checksum:08x}; {time.monotonic()-started:.2f}s')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('file', type=Path)
    parser.add_argument('--address', default='10.0.2.15')
    parser.add_argument('--verify-only', action='store_true', help='Read the stored file without writing; also works after a reboot')
    args = parser.parse_args()
    upload(args.file, args.address, args.verify_only)


if __name__ == '__main__':
    main()
