#!/usr/bin/env python3
"""Run a Wi-Fi hardware check in Pi RAM and require return to the installed kernel."""
import argparse
import importlib.util
import os
from pathlib import Path
import subprocess
import sys

__test__ = False  # Physical test, invoked explicitly with the attached Pi.
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('uart', ROOT / 'scripts/upload-pi2-kernel.py')
uart = importlib.util.module_from_spec(spec)
spec.loader.exec_module(uart)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('kernel', type=Path)
    parser.add_argument('--uart', default='/dev/cu.usbserial-0001')
    parser.add_argument('--timeout', type=float, default=180)
    parser.add_argument('--from-loader', action='store_true', help='Loader is already waiting after an earlier diagnostic')
    parser.add_argument('--stay-in-loader', action='store_true', help='Hold UART recovery after the test for another RAM diagnostic')
    args = parser.parse_args()
    image = uart.build_upload(args.kernel.read_bytes())
    if not args.from_loader:
        subprocess.run([
        sys.executable, str(ROOT / 'scripts/upload-pi2-user.py'), args.uart,
        str(ROOT / 'target/arm_reboot.elf'), '--retries', '1', '--wait-for-reboot',
        ], check=True)
    fd = uart.open_uart(args.uart)
    try:
        uart.write_all(fd, image, 256, 500)
        # Retain the entire experiment output: the generic wait helper bounds
        # its buffer and could discard the result during the subsequent boot.
        import select
        import time
        output = bytearray()
        deadline = time.monotonic() + args.timeout
        while time.monotonic() < deadline:
            readable, _, _ = select.select([fd], [], [], 0.2)
            if not readable:
                continue
            data = os.read(fd, 4096)
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
            output.extend(data)
            if args.stay_in_loader and b'GABIX LOADER READY' in output:
                # Keep the paced path: macOS tcdrain can hang after an upload.
                uart.write_all(fd, b'\x00', 1, 500)
                print('UART loader held for the next diagnostic.')
                break
            if b'GABIX USER READY' in output:
                break
        if not args.stay_in_loader and b'GABIX USER READY' not in output:
            raise RuntimeError('Installed kernel did not resume before the deadline')
        if b'WIFI CHECK PASS' not in output or b'WIFI CHECK FAILED' in output:
            raise RuntimeError('Wi-Fi hardware check failed; see UART output above')
        print('Physical Wi-Fi check passed; ' + ('UART loader waiting.' if args.stay_in_loader else 'installed kernel resumed.'))
    finally:
        os.close(fd)


if __name__ == '__main__':
    main()
