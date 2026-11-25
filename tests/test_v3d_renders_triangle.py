#!/usr/bin/env python3
"""Run on a physical Pi 2B booted with pi2-wifi-dev. Results go to the specified directory."""
import argparse
import json
from pathlib import Path
import os
import struct
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / "scripts"
BLUE = bytes.fromhex("964219ff")
GREEN = bytes.fromhex("418118ff")
GOLD = bytes.fromhex("28cdffff")


def boot_status(address):
    output = subprocess.check_output(
        [sys.executable, SCRIPTS / "pi2-manage.py", "status", "--address", address]
    )
    return json.loads(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--address", default="gabix-pi2.local")
    parser.add_argument("--out-dir", required=True, type=Path)
    args = parser.parse_args()
    if os.environ.get("GABIX_PI_LEGACY") == "1":
        parser.error("The physical development runner requires authenticated management")
    out = args.out_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for name in ("timer", "v3d"):
        subprocess.run(
            ["sh", SCRIPTS / "build-pi2-ram-probe.sh",
             ROOT / "targets/armv7/probes" / f"{name}.rs", out / f"{name}.img"],
            check=True, cwd=ROOT,
        )

    def run(source, data, filename):
        flag = "--image" if isinstance(source, Path) else "--session"
        reply = subprocess.check_output(
            [sys.executable, SCRIPTS / "pi2-ram-probe.py", "--address", args.address,
             flag, str(source), "--input-hex", data.hex(), "--output", out / filename],
            text=True,
        )
        return int(reply.splitlines()[0].removeprefix("Session ")), (out / filename).read_bytes()

    before = boot_status(args.address)
    assert before["authentication_enabled"] == 1
    timer = out / "timer.img"
    for delay in (1000, 2000):
        timer, result = run(timer, struct.pack("<I", delay), "timer.bin")
        start, end = struct.unpack("<II", result)
        elapsed = (end - start) & 0xffffffff
        assert elapsed >= delay, (delay, elapsed)
        print(f"Timer requested {delay} us, measured {elapsed} us")
    gpu, ident = run(out / "v3d.img", b"\x00", "ident.bin")
    assert struct.unpack("<I", ident) == (0x02443356,)
    for name, background in (("blue", BLUE), ("green", GREEN)):
        _, frame = run(gpu, b"\x01" + background, f"clear-{name}.bin")
        assert frame == background * 4096, f"{name} clear failed"

    alternate = bytes.fromhex("fc9929ff")
    for name, background, foreground in (("triangle", BLUE, GOLD),
                                         ("triangle-green", GREEN, alternate)):
        started = time.monotonic()
        inputs = b"\x02" + background + (foreground if foreground != GOLD else b"")
        _, frame = run(gpu, inputs, f"{name}.bin")
        duration = time.monotonic() - started
        assert len(frame) == 64 * 64 * 4
        for y in range(64):
            for x in range(64):
                # Test the pixel center against the two sloping edges and the base.
                inside = 8 <= y < 56 and 2 * abs(2 * x + 1 - 64) < 2 * y + 1 - 16
                offset = (y * 64 + x) * 4
                pixel = frame[offset:offset + 4]
                assert pixel == (foreground if inside else background), (name, x, y, pixel.hex())
        print(f"{name} verified all 4096 pixels, draw and Wi-Fi readback {duration:.3f} s")
    after = boot_status(args.address)
    assert before["boot"] == after["boot"], (before["boot"], after["boot"])
    assert before["kernel_crc32"] == after["kernel_crc32"]
    print(f"Boot {after['boot']}, kernel CRC32 {after['kernel_crc32']:08x}, V3D_IDENT0 0x02443356")
    print(f"V3D session {gpu}")


if __name__ == "__main__":
    main()
