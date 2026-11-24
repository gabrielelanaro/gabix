#!/usr/bin/env python3
"""Physical Pi acceptance check for an installed Wi-Fi user loader."""
import argparse
import json
import os
import unittest
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def check_board(address):
    with tempfile.TemporaryDirectory() as tmp:
        programs = []
        for name in ("arm_syscall", "arm_report_mode"):
            output = Path(tmp) / f"{name}.elf"
            subprocess.run(["sh", "scripts/build-arm-user.sh", f"tests/fixtures/{name}.rs", output],
                           cwd=ROOT, check=True)
            programs.append(output)
        result = subprocess.check_output(
            [sys.executable, "scripts/pi2-wifi-user.py", "--address", address, *programs],
            cwd=ROOT, text=True)
    records = [json.loads(line) for line in result.splitlines()]
    assert [r["elf"] for r in records] == [p.name for p in programs]
    assert [r["exit"] for r in records] == [0, 0x10], records
    assert len({(r["boot"], r["kernel_crc32"]) for r in records}) == 1, records
    print(result, end="")


@unittest.skipUnless(os.environ.get("GABIX_PI_TEST_ADDRESS"), "physical Pi test")
def test_upload_runs_two_user_programs():
    check_board(os.environ["GABIX_PI_TEST_ADDRESS"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--address", default="gabix-pi2.local")
    check_board(parser.parse_args().address)
