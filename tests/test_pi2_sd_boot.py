#!/usr/bin/env python3
"""Opt-in physical SD boot checks; artifacts go to the supplied run directory.

The card must already have the documented two-copy layout and installer service.
RAM mode keeps the installed loader intact. Installed mode exercises watchdog
fallback without another host upload. The original working copy stays intact.
"""

import argparse
import binascii
import importlib.util
import os
from pathlib import Path
import struct
import subprocess
import sys

# Physical writes require the explicit command-line invocation below.
__test__ = False

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "install", ROOT / "scripts/pi2-install.py"
)
install = importlib.util.module_from_spec(spec)
spec.loader.exec_module(install)
uart = install.load_module("uart", ROOT / "scripts/upload-pi2-kernel.py")
UART = "/dev/cu.usbserial-0001"
IMAGES = ROOT / "targets/armv7/target/armv7a-none-eabihf/release"


def run(script, *args):
    subprocess.run(
        [sys.executable, str(ROOT / "scripts" / script), *map(str, args)], check=True
    )


def reboot():
    run(
        "upload-pi2-user.py",
        UART,
        ROOT / "target/arm_reboot.elf",
        "--retries",
        "1",
        "--wait-for-reboot",
    )


def upload(image, marker=b"GABIX USER READY"):
    fd = uart.open_uart(UART)
    try:
        uart.write_all(fd, uart.build_upload(image.read_bytes()), 256, 500)
        accepted, output = uart.wait_for_result(fd, 15)
        if not accepted:
            raise RuntimeError("UART upload rejected")
        if marker not in output:
            output += uart.wait_for(fd, marker, 150)
        return output
    finally:
        os.close(fd)


def listen(marker):
    fd = uart.open_uart(UART)
    try:
        return uart.wait_for(fd, marker, 150)
    finally:
        os.close(fd)


def checksum(record):
    struct.pack_into("<I", record, 508, binascii.crc32(record[:508]))
    return bytes(record)


def patch(args, name, mutate, kernel=None):
    card = install.Card(args.address)
    try:
        old = card.read(install.START)
        record = bytearray(old)
        active = struct.unpack_from("<I", record, 12)[0]
        target = 1 - active
        patches = []
        if kernel is not None:
            offset = 16 + 24 * target
            start = install.START + struct.unpack_from("<Q", record, offset)[0]
            install.add_bytes(patches, card.read, start, kernel)
            struct.pack_into(
                "<II", record, offset + 16, len(kernel), binascii.crc32(kernel)
            )
            struct.pack_into("<III", record, 64, target, 0, install.NONE)
        mutate(record)
        patches.append((install.START, old, checksum(record)))
        package = args.output / (name + ".pkg")
        install.write_package(package, patches)
    finally:
        card.close()
    install.apply(argparse.Namespace(package=package, address=args.address))
    return old


def boot(args, marker=b"GABIX USER READY"):
    reboot()
    if args.ram:
        return upload(IMAGES / "kernel-pi2-loader-trial.img", marker)
    return listen(marker)


def test_hung_trial_boots_previous(args):
    # ARM instructions: disable IRQ/FIQ; WFE; branch back to WFE.
    hung = struct.pack("<III", 0xF10C00C0, 0xE320F002, 0xEAFFFFFD)
    if args.ram:
        patch(args, "hung", lambda record: None, hung)
    else:
        image = args.output / "hung.img"
        image.write_bytes(hung)
        install.client.upload(image, args.address, kernel=True)
    if args.ram:
        output = boot(args, b"GABIX LOADER READY")
        # First READY may be the RAM loader itself. Wait for reset if necessary.
        if b"GABIX SD KERNEL VERIFIED" not in output:
            output += listen(b"GABIX LOADER READY")
        assert b"GABIX SD KERNEL VERIFIED" in output
        output = upload(IMAGES / "kernel-pi2-loader-trial.img")
    else:
        output = boot(args)
        assert output.count(b"GABIX SD KERNEL VERIFIED") == 2, output
    assert b"GABIX SD BOOT CONFIRMED" in output, output
    print("PASS: hung trial reset and previous kernel confirmed")


def test_candidate_boots_and_confirms(args):
    image = args.kernel
    if args.ram:
        patch(args, "candidate", lambda record: None, image.read_bytes())
    else:
        install.client.upload(image, args.address, kernel=True)
    output = boot(args)
    assert b"GABIX SD BOOT CONFIRMED" in output, output
    print("PASS: candidate booted from SD and confirmed")


def test_invalid_copies_accept_uart_recovery(args):
    def corrupt(record):
        for offset in (36, 60):
            struct.pack_into(
                "<I", record, offset, struct.unpack_from("<I", record, offset)[0] ^ 1
            )
        struct.pack_into("<III", record, 64, install.NONE, 0, install.NONE)

    good = patch(args, "invalid-copies", corrupt)
    (args.output / "good-record.bin").write_bytes(good)
    output = boot(args, b"GABIX SD COPIES EXHAUSTED")
    assert output.count(b"GABIX SD COPY INVALID") == 2, output
    upload(args.kernel)
    patch(args, "restore-record", lambda record: record.__setitem__(slice(None), good))
    output = boot(args)
    assert b"GABIX SD BOOT CONFIRMED" in output, output
    print("PASS: both invalid copies entered UART recovery; restored SD boot")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("case", choices=["all", "hung", "candidate", "recovery"])
    p.add_argument("--ram", action="store_true")
    p.add_argument("--address", default="10.0.2.15")
    p.add_argument("--kernel", type=Path, default=IMAGES / "kernel-pi2-managed.img")
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    cases = {
        "hung": test_hung_trial_boots_previous,
        "candidate": test_candidate_boots_and_confirms,
        "recovery": test_invalid_copies_accept_uart_recovery,
    }
    selected = cases.values() if args.case == "all" else [cases[args.case]]
    for check in selected:
        check(args)


if __name__ == "__main__":
    main()
