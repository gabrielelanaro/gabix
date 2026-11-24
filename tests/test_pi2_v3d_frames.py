#!/usr/bin/env python3
"""Upload the triangle server and compare two complete V3D frames on one connection."""
import argparse
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import time
import unittest
import zlib

from test_pi2_wifi_tcp_echo import ROOT, client


def receive_frame(stream, background, triangle, slow):
    stream.sendall(background + triangle)
    expected = b"".join(
        triangle if 8 <= y < 56 and 2 * abs(2*x + 1 - 64) < 2*y + 1 - 16 else background
        for y in range(64) for x in range(64)
    )
    result = bytearray()
    if slow:
        time.sleep(2)
    while len(result) < len(expected):
        chunk = stream.recv(min(73 if slow else 16384, len(expected) - len(result)))
        if not chunk:
            raise EOFError(f"Frame stopped at {len(result)} bytes")
        result.extend(chunk)
        if slow:
            time.sleep(0.015)
    if result != expected:
        offset = next(i for i, pair in enumerate(zip(result, expected)) if pair[0] != pair[1])
        raise AssertionError(f"Frame mismatch at byte {offset}: {result[offset]} != {expected[offset]}")
    print(f"{len(result)} bytes matched, CRC32 {zlib.crc32(result):08x}, slow={slow}", flush=True)
    return expected


def check_loss(pi, before, expected):
    for _ in range(10):
        count = pi.status()["newest_log"] - before["newest_log"]
        lines = "\n".join(record["text"] for record in pi.logs(count) if record["boot"] == before["boot"])
        entries = re.findall(r"frame (drop|retry) (\d+) (\d+) ([0-9a-f]{8})", lines)
        if len(entries) >= 2:
            drop, retry = entries
            assert drop[0] == "drop" and retry[0] == "retry" and drop[1:] == retry[1:], entries
            size = int(drop[2])
            assert 0 < size <= 536 and int(drop[3], 16) == zlib.crc32(expected[:size]), entries
            print(f"Pi withheld and retransmitted sequence {drop[1]}, {size} bytes, CRC32 {drop[3]}")
            return
        time.sleep(1)
    raise AssertionError("Missing matching Pi drop/retry records")


def check_board(address, expect_drop):
    with tempfile.TemporaryDirectory() as tmp:
        elf = Path(tmp) / "frames.elf"
        subprocess.run(["sh", "scripts/build-arm-user.sh", "tests/fixtures/arm_v3d_frames.rs", elf],
                       cwd=ROOT, check=True)
        pi = client.management.Pi(address)
        try:
            before = pi.status()
            session = client.start(pi, elf)
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as stream:
                stream.settimeout(20)
                stream.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 2048)
                stream.connect((address, 18081))
                print(f"Boot {before['boot']}, stream {stream.getsockname()} -> {stream.getpeername()}", flush=True)
                receive_frame(stream, bytes.fromhex("964219ff"), bytes.fromhex("28cdffff"), False)
                second = receive_frame(stream, bytes.fromhex("418118ff"), bytes.fromhex("fc9929ff"), True)
                stream.shutdown(socket.SHUT_WR)
                assert stream.recv(1) == b""
            assert client.wait_for_exit(pi, session) == 0
            after = pi.status()
            assert (before["boot"], before["kernel_crc32"]) == (after["boot"], after["kernel_crc32"])
            if expect_drop:
                check_loss(pi, before, second)
            print(f"Uploaded ELF exited 0, kernel {after['kernel_crc32']:08x}")
        finally:
            pi.close()


@unittest.skipUnless(os.environ.get("GABIX_PI_TEST_ADDRESS"), "physical Pi test")
def test_request_transfers_two_complete_frames():
    check_board(os.environ["GABIX_PI_TEST_ADDRESS"], os.getenv("GABIX_PI_FRAME_LOSS_TEST") == "1")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--address", default="gabix-pi2.local")
    parser.add_argument("--expect-drop", action="store_true",
                        help="Require logs from a GABIX_PI_FRAME_LOSS_TEST=1 kernel")
    args = parser.parse_args()
    check_board(args.address, args.expect_drop)
