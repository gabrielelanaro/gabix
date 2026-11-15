#!/usr/bin/env python3
"""Reboot a running UART-enabled Pi kernel and check process isolation on hardware.

Build pi2-dev and target/arm_reboot.elf first. The boot checks take about a minute
on the Pi because the test programs repeatedly access noncacheable user memory.
"""
import argparse
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("uart")
    parser.add_argument("--image", type=Path, default=ROOT / "targets/armv7/target/armv7a-none-eabihf/release/kernel-pi2-dev.img")
    parser.add_argument("--reboot-elf", type=Path, default=ROOT / "target/arm_reboot.elf")
    args = parser.parse_args()
    for path in (args.image, args.reboot_elf):
        if not path.is_file():
            parser.error(f"missing build artifact: {path}")
    logs = Path(tempfile.mkdtemp(prefix="gabix-pi2-isolation-"))
    print(f"Physical check logs: {logs}", flush=True)

    def run(name, command):
        log = logs / f"{name}.log"
        print(f"{name}…", flush=True)
        with log.open("w") as output:
            result = subprocess.run(command, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT)
        if result.returncode:
            raise RuntimeError(f"{name} failed; inspect {log}")
        return log.read_text(errors="replace")

    run("uart-reboot", [sys.executable, "scripts/upload-pi2-user.py", args.uart,
        str(args.reboot_elf), "--retries", "1", "--wait-for-reboot"])
    boot = run("isolation-boot", [sys.executable, "scripts/upload-pi2-kernel.py", args.uart,
        str(args.image), "--inter-byte-delay-us", "500", "--retries", "1",
        "--wait-for-user-ready", "--user-ready-timeout", "90"])
    for marker in ("gabix arm isolated userspace processes passed",
                   "gabix arm user faults contained; peer process survived"):
        if marker not in boot:
            raise RuntimeError(f"missing marker {marker!r}; inspect {logs / 'isolation-boot.log'}")
    run("udp-echo", [sys.executable, "scripts/pi2-echo.py", "isolated processes passed"])
    print("PASS: userspace isolation, fault containment, UART reload, and UDP echo", flush=True)


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as error:
        print(error, file=sys.stderr)
        sys.exit(1)
