#!/usr/bin/env python3
"""Physical Pi test: stream a file to the reserved SD region and read it back.

Requires the pi2-sd-transfer kernel. --verify-only checks the same file after
UART reboot/re-upload, without sending its bytes to the Pi again.
"""
import argparse
import binascii
import importlib.util
from pathlib import Path
import secrets
import socket

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('sd_client', ROOT / 'scripts/pi2-sd-transfer.py')
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)


def test_ethernet_stores_file(address, verify_only):
    data = bytes((i*37+i//251+11) % 256 for i in range(65549))
    checksum = binascii.crc32(data)
    session = secrets.randbelow(0xffffffff) + 1
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.connect((address, 5556))
        exchange = lambda op, arg=0, val=0, body=b'': client.exchange(connection, session, op, arg, val, body)
        if not verify_only:
            exchange(1, len(data), checksum)
            # Reordering and corrupt chunks must leave the next sector untouched.
            for offset, crc, expected_status in [(512, binascii.crc32(data[512:1024]), 2), (0, 0, 4)]:
                try:
                    exchange(2, offset, crc, data[offset:offset+512])
                    raise AssertionError('invalid chunk was accepted')
                except RuntimeError as error:
                    assert str(error).endswith(f'status {expected_status}'), error
            for offset in range(0, len(data), 512):
                chunk = data[offset:offset+512]
                exchange(2, offset, binascii.crc32(chunk), chunk)
                if offset == 0:
                    # Repeating an acknowledged chunk exercises lost-ACK recovery.
                    exchange(2, offset, binascii.crc32(chunk), chunk)
            exchange(3)
            exchange(3)  # A retried finish must preserve the completed file.
        metadata = exchange(5)
        assert metadata == client.struct.pack('<II', len(data), checksum)
        for offset in range(0, len(data), 512):
            assert exchange(4, offset) == data[offset:offset+512], offset
    print(f'PASS: {len(data)} bytes read from SD exactly' + (' after reboot' if verify_only else '; retry/order/CRC checks passed'))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--address', default='10.0.2.15')
    parser.add_argument('--verify-only', action='store_true')
    args = parser.parse_args()
    test_ethernet_stores_file(args.address, args.verify_only)
