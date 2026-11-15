#!/usr/bin/env python3
"""Read Pi status/logs or reboot through authenticated UDP management."""

import argparse
import binascii
import importlib.util
import json
from pathlib import Path
import secrets
import socket
import struct

spec = importlib.util.spec_from_file_location(
    "transfer", Path(__file__).with_name("pi2-sd-transfer.py")
)
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)


class Pi:
    def __init__(self, address):
        self.connection = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.connection.connect((address, 5556))
        self.session = secrets.randbelow(0xFFFFFFFF) + 1

    def request(self, operation, argument=0, value=0):
        return client.exchange(
            self.connection, self.session, operation, argument, value
        )

    def status(self):
        b = self.request(11)
        if len(b) != 176 or struct.unpack_from("<I", b)[0] != 1:
            raise ValueError("Unknown status format")
        u32 = lambda n: struct.unpack_from("<I", b, n)[0]
        u64 = lambda n: struct.unpack_from("<Q", b, n)[0]
        result = {
            name: u32(n)
            for name, n in [
                ("dhcp_attempts", 40),
                ("network_tx_failures", 44),
                ("received_frames", 48),
                ("rejected_dhcp", 52),
                ("lease_losses", 56),
                ("lease_seconds", 60),
                ("log_write_errors", 104),
                ("dropped_log_lines", 108),
                ("corrupt_log_records", 112),
                ("authentication_rejections", 116),
                ("management_requests", 120),
                ("management_tx_failures", 124),
                ("active_copy", 136),
                ("pending_copy", 140),
                ("attempted_mask", 144),
                ("reset_status_raw", 148),
                ("kernel_crc32", 152),
                ("kernel_limit_bytes", 156),
                ("log_capacity_sectors", 160),
                ("authentication_enabled", 164),
            ]
        }
        result.update(
            {
                name: u64(n)
                for name, n in [
                    ("lease_remaining_seconds", 64),
                    ("uptime_ms", 72),
                    ("boot", 80),
                    ("newest_log", 88),
                    ("oldest_log", 96),
                    ("last_request_ms", 128),
                ]
            }
        )
        result.update(
            {
                name: ".".join(map(str, b[n : n + 4]))
                for name, n in [
                    ("ip", 12),
                    ("subnet_mask", 16),
                    ("gateway", 20),
                    ("dhcp_server", 24),
                    ("dns", 28),
                ]
            }
        )
        result["mac"] = ":".join(f"{x:02x}" for x in b[32:38])
        result["dhcp_state"] = [
            "discovering",
            "requesting",
            "bound",
            "renewing",
            "rebinding",
        ][u32(4)]
        result["link"] = ["unknown", "down", "up"][u32(8)]
        if result["pending_copy"] == 0xFFFFFFFF:
            result["pending_copy"] = None
        return result

    def logs(self, limit=128):
        status = self.status()
        first = max(status["oldest_log"], status["newest_log"] - limit + 1)
        for sequence in range(first, status["newest_log"] + 1):
            b = self.request(12, sequence & 0xFFFFFFFF, sequence >> 32)
            if (
                len(b) != 512
                or b[:8] != b"GBXLOG01"
                or binascii.crc32(b[:508]) != struct.unpack_from("<I", b, 508)[0]
            ):
                raise ValueError(f"Invalid log record {sequence}")
            number, boot, milliseconds = struct.unpack_from("<QQQ", b, 8)
            length = struct.unpack_from("<H", b, 32)[0]
            if number != sequence or length > 468:
                raise ValueError("Log record bounds mismatch")
            yield {
                "sequence": number,
                "boot": boot,
                "uptime_ms": milliseconds,
                "text": b[40 : 40 + length].decode("utf-8", errors="replace"),
            }

    def close(self):
        self.connection.close()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("command", choices=["status", "logs", "reboot"])
    p.add_argument("--address", default="10.0.2.15")
    p.add_argument("--records", type=int, default=128)
    p.add_argument("--json", action="store_true")
    args = p.parse_args()
    if not 1 <= args.records <= 32768:
        p.error("--records must be 1..32768")
    pi = Pi(args.address)
    try:
        if args.command == "status":
            print(json.dumps(pi.status(), indent=2))
        elif args.command == "logs":
            records = list(pi.logs(args.records))
            if args.json:
                print(json.dumps(records, indent=2))
            else:
                pending = ""
                previous_boot = None
                for record in records:
                    if previous_boot != record["boot"] and pending:
                        print(pending + " [partial]")
                        pending = ""
                    previous_boot = record["boot"]
                    pending += record["text"]
                    while "\n" in pending:
                        line, pending = pending.split("\n", 1)
                        print(
                            f"[boot {record['boot']} +{record['uptime_ms']/1000:.3f}s] {line}"
                        )
                if pending:
                    print(pending + " [partial]")
        else:
            pi.request(13)
            print("Reboot acknowledged; the Pi will restart in one second")
    finally:
        pi.close()


if __name__ == "__main__":
    main()
