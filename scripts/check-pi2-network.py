#!/usr/bin/env python3
"""Upload the physical Pi kernel and check Ethernet communication after idle.

Requires the Pi at GABIX LOADER READY, a release pi2-network-dev image,
and the Mac DHCP peer from pi2-dhcp-peer-macos.sh on the direct Ethernet link.
The UDP echo service currently runs in the kernel.
"""

import argparse
import importlib.util
import os
from pathlib import Path
import select
import socket
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "pi2_uploader", ROOT / "scripts/upload-pi2-kernel.py"
)
UPLOADER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UPLOADER)


def check_network(args, results):
    try:
        print("\nDHCP complete. Checking ping and ARP...", flush=True)
        ping = subprocess.run(
            ["ping", "-n", "-c", "2", "-W", "700", args.address],
            capture_output=True, text=True, timeout=4,
        )
        print(ping.stdout, end="", flush=True)
        results["ping"] = ping.returncode == 0 and "2 packets received" in ping.stdout
        arp = subprocess.run(
            ["arp", "-n", args.address], capture_output=True, text=True, timeout=2,
        )
        print("ARP:", arp.stdout.strip(), flush=True)
        results["arp"] = (
            arp.returncode == 0 and " at " in arp.stdout
            and "incomplete" not in arp.stdout
        )
        # These pauses force empty USB receive polls between network requests.
        # The old driver wedged after NAK and could answer only the first ping.
        replies = 0
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
            connection.bind(("10.0.2.2", 0))
            connection.connect((args.address, 5555))
            connection.settimeout(1)
            for index in range(3):
                time.sleep(1)
                message = args.message if index == 0 else f"{args.message} [{index + 1}]"
                payload = message.encode("utf-8")
                print(f"Mac → Pi: {message}", flush=True)
                try:
                    connection.send(payload)
                    reply = connection.recv(1500)
                    print(f"Pi → Mac: {reply.decode('utf-8', errors='replace')}", flush=True)
                    if reply == payload:
                        replies += 1
                    else:
                        print("FAIL: reply differs from the sent bytes", flush=True)
                except OSError as error:
                    print(f"UDP failed: {error}", flush=True)
        results["udp_after_idle"] = replies == 3
        print(f"UDP: {replies}/3 exact replies", flush=True)
    except (OSError, subprocess.TimeoutExpired) as error:
        print(f"Network checks failed: {error}", flush=True)
    finally:
        print("Checks:", results, flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--uart", default="/dev/cu.usbserial-0001")
    parser.add_argument("--address", default="10.0.2.15")
    parser.add_argument("--message", default="Hello Pi! This is the Mac.")
    parser.add_argument(
        "--timeout", type=float, default=180,
        help="Seconds after upload for boot checks, DHCP, echo and watchdog return",
    )
    parser.add_argument(
        "--kernel", type=Path,
        default=ROOT / "targets/armv7/target/armv7a-none-eabihf/release/kernel-pi2-network-dev.img",
    )
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("timeout must be positive")
    if len(args.message.encode("utf-8")) > 1000:
        parser.error("message must fit in 1000 UTF-8 bytes")
    kernel = args.kernel.read_bytes()
    results = {"ping": False, "arp": False, "udp_after_idle": False}
    worker = None
    fd = UPLOADER.open_uart(args.uart)
    uploaded = False
    ready = False
    try:
        print(f"Uploading {len(kernel):,} bytes to the Pi...", flush=True)
        UPLOADER.write_all(fd, UPLOADER.build_upload(kernel), 4096, 500)
        # Physical process-isolation checks alone take about 50 seconds.
        # Leave time for the rest of boot, link negotiation and the watchdog.
        deadline = time.monotonic() + args.timeout
        received = bytearray()
        pending = bytearray()
        while time.monotonic() < deadline:
            if not select.select([fd], [], [], 0.1)[0]:
                continue
            chunk = os.read(fd, 8192)
            received.extend(chunk)
            pending.extend(chunk)
            while b"\n" in pending:
                line, _, tail = pending.partition(b"\n")
                pending = bytearray(tail)
                print("Pi UART:", line.decode(errors="replace").rstrip("\r"), flush=True)
            uploaded = b"UPLOAD OK" in received
            if uploaded and b"dhcp configured" in received and worker is None:
                worker = threading.Thread(target=check_network, args=(args, results))
                worker.start()
            if uploaded and b"GABIX LOADER READY" in received:
                ready = True
                break
            if b"UPLOAD FAILED" in received:
                break
        if worker:
            worker.join()
        if not ready:
            print(f"FAIL: no loader-ready marker within {args.timeout:g} seconds after upload.")
        elif worker is None:
            print("FAIL: kernel did not complete DHCP; check the Mac DHCP peer and cable.")
        passed = ready and all(results.values())
        print("PASS: ARP, ping, and three UDP replies after idle" if passed else "FAIL: network checks incomplete")
        return 0 if passed else 1
    finally:
        os.close(fd)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        print(f"Network check failed: {error}", file=sys.stderr)
        raise SystemExit(1)
