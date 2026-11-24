#!/usr/bin/env python3
"""Run an uploaded ARM echo server on the physical Pi and check a single TCP stream."""
import argparse
import importlib.util
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("wifi_user", ROOT / "scripts/pi2-wifi-user.py")
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)
IDLE_SECONDS = 12   # Longer than the ten-second user execution watchdog.


def echo(stream, data):
    stream.sendall(data)
    received = bytearray()
    while len(received) < len(data):
        part = stream.recv(len(data) - len(received))
        if not part:
            raise EOFError("server closed before completing the echo")
        received.extend(part)
    assert received == data


def check_board(address):
    if os.getenv("GABIX_PI_LEGACY") == "1":
        raise ValueError("Authenticated Wi-Fi management is required")
    with tempfile.TemporaryDirectory() as tmp:
        elf = Path(tmp) / "arm_tcp_echo.elf"
        subprocess.run(["sh", "scripts/build-arm-user.sh", "tests/fixtures/arm_tcp_echo.rs", elf],
                       cwd=ROOT, check=True)
        pi = client.management.Pi(address)
        try:
            initial = pi.status()
            session = client.start(pi, elf)
            # Exercise management while the ELF is inside accept().
            assert pi.status()["boot"] == initial["boot"]
            assert client.management.client.exchange(pi.connection, session, client.RESULT) == b""
            with socket.create_connection((address, 8080), timeout=10) as stream:
                first = b"first message from the Mac\n"
                second = bytes(range(256)) + b"second message after the idle gap\n"
                echo(stream, first)
                time.sleep(IDLE_SECONDS)
                during = pi.status()
                assert during["boot"] == initial["boot"] and during["link"] == "up"
                assert client.management.client.exchange(pi.connection, session, client.RESULT) == b""
                echo(stream, second)
                print(f"Boot {during['boot']}, TCP {stream.getsockname()} -> {stream.getpeername()}, "
                      f"echoed {len(first)} and {len(second)} bytes across {IDLE_SECONDS}s idle", flush=True)
                stream.shutdown(socket.SHUT_WR)
                assert stream.recv(1) == b""
            assert client.wait_for_exit(pi, session) == 0
            final = pi.status()
            assert (final["boot"], final["kernel_crc32"]) == (initial["boot"], initial["kernel_crc32"])
            print(f"User ELF exited 0; kernel CRC32 {final['kernel_crc32']:08x}")
        finally:
            pi.connection.close()


@unittest.skipUnless(os.environ.get("GABIX_PI_TEST_ADDRESS"), "physical Pi test")
def test_read_echoes_two_messages_on_one_connection():
    check_board(os.environ["GABIX_PI_TEST_ADDRESS"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--address", default="gabix-pi2.local")
    check_board(parser.parse_args().address)
