#!/usr/bin/env python3
"""Explicit physical management checks; requires a reachable managed Pi."""

import argparse
import importlib.util
import json
from pathlib import Path
import socket
import struct
import time

__test__ = False
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("manage", ROOT / "scripts/pi2-manage.py")
manage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manage)


class RecordingSocket:
    def __init__(self, connection):
        self.connection = connection
        self.last_packet = None

    def send(self, packet):
        self.last_packet = packet
        return self.connection.send(packet)

    def recv(self, n):
        return self.connection.recv(n)

    def settimeout(self, t):
        self.connection.settimeout(t)

    def close(self):
        self.connection.close()


def silent(connection, packet):
    connection.send(packet)
    connection.settimeout(0.5)
    try:
        connection.recv(1024)
    except socket.timeout:
        return
    raise AssertionError("An unauthenticated or stale request received a reply")


def test_authentication_rejects_unsigned_and_modified_requests(address):
    pi = manage.Pi(address)
    try:
        before = pi.status()
        assert before["authentication_enabled"] == 1
        pi.connection = RecordingSocket(pi.connection)
        first = pi.status()
        packet = pi.connection.last_packet
        silent(pi.connection, manage.client.HEADER.pack(b"GBSD", 11, pi.session, 0, 0))
        changed = bytearray(packet)
        changed[-1] ^= 1
        silent(pi.connection, changed)
        # The most recent valid request is retryable without another execution.
        pi.connection.send(packet)
        response = pi.connection.recv(1024)
        assert response[:4] == b"GBAU"
        after = pi.status()
        assert after["management_requests"] == first["management_requests"] + 1
        assert (
            after["authentication_rejections"]
            >= before["authentication_rejections"] + 2
        )
        print(
            "PASS: unsigned/modified requests rejected; duplicate retry did not execute again",
            flush=True,
        )
    finally:
        pi.close()


def test_network_reboot_preserves_logs(address, output):
    pi = manage.Pi(address)
    try:
        before = pi.status()
        old_records = list(pi.logs(16))
        assert old_records
        pi.connection = RecordingSocket(pi.connection)
        pi.request(13)
        reboot_packet = pi.connection.last_packet
    finally:
        pi.close()
    deadline = time.monotonic() + 200
    while time.monotonic() < deadline:
        time.sleep(3)
        pi = manage.Pi(address)
        try:
            after = pi.status()
            if after["boot"] <= before["boot"]:
                continue
            assert after["ip"] != "0.0.0.0" and after["link"] == "up"
            logs = list(pi.logs(64))
            assert any(
                r["boot"] == before["boot"]
                and "management reboot requested" in r["text"]
                for r in logs
            )
            assert any(
                r["boot"] == after["boot"] and "GABIX SD BOOT CONFIRMED" in r["text"]
                for r in logs
            )
            silent(pi.connection, reboot_packet)
            assert pi.status()["boot"] == after["boot"]
            (output / "status.json").write_text(json.dumps(after, indent=2) + "\n")
            (output / "logs.json").write_text(json.dumps(logs, indent=2) + "\n")
            print(
                "PASS: network reboot, DHCP recovery, previous-boot logs, and cross-boot replay rejection",
                flush=True,
            )
            return
        except OSError:
            pass
        finally:
            pi.close()
    raise TimeoutError("Pi did not return after authenticated reboot")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("case", choices=["authentication", "reboot"])
    p.add_argument("--address", default="10.0.2.15")
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    if args.case == "authentication":
        test_authentication_rejects_unsigned_and_modified_requests(args.address)
    else:
        test_network_reboot_preserves_logs(args.address, args.output)


if __name__ == "__main__":
    main()
