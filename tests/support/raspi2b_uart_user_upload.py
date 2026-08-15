#!/usr/bin/env python3
"""Drive the public Pi 2 UART userspace-upload lifecycle under QEMU."""

import argparse
import importlib.util
import os
import select
import subprocess
import sys
import time
from pathlib import Path


USER_READY = b"GABIX USER READY"
USER_UPLOAD_OK = b"USER UPLOAD OK"
USER_UPLOAD_TIMEOUT = b"USER UPLOAD TIMEOUT"
USER_OUTPUT = b"gabix arm userspace wrote"


def load_uploader(repo_root: Path):
    path = repo_root / "scripts" / "upload-pi2-kernel.py"
    spec = importlib.util.spec_from_file_location("pi2_uploader", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read_until(process, output: bytearray, predicate, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"QEMU exited with status {process.returncode}")
        readable, _, _ = select.select(
            [process.stdout], [], [], min(0.2, deadline - time.monotonic())
        )
        if not readable:
            continue
        chunk = os.read(process.stdout.fileno(), 4096)
        if not chunk:
            continue
        output.extend(chunk)
        if predicate(bytes(output)):
            return
    raise TimeoutError("timed out waiting for UART userspace lifecycle")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("qemu")
    parser.add_argument("kernel", type=Path)
    parser.add_argument("user_elf", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parents[2]
    uploader = load_uploader(repo_root)
    process = subprocess.Popen(
        [
            args.qemu,
            "-M",
            "raspi2b",
            "-kernel",
            str(args.kernel),
            "-display",
            "none",
            "-audio",
            "none",
            "-monitor",
            "none",
            "-serial",
            "stdio",
            "-no-reboot",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        bufsize=0,
    )
    output = bytearray()
    try:
        read_until(process, output, lambda data: USER_READY in data, 10)
        process.stdin.write(b"GABIXLOAD\x01\x00")
        process.stdin.flush()
        read_until(
            process,
            output,
            lambda data: USER_UPLOAD_TIMEOUT in data and data.count(USER_READY) >= 2,
            4,
        )
        upload = uploader.build_upload(args.user_elf.read_bytes())
        for run in range(1, 3):
            process.stdin.write(upload)
            process.stdin.flush()
            read_until(
                process,
                output,
                lambda data, run=run: data.count(USER_UPLOAD_OK) >= run
                and data.count(USER_OUTPUT) >= run
                and data.count(USER_READY) >= run + 2,
                10,
            )
    finally:
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        args.output.write_bytes(output)

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, TimeoutError) as error:
        print(error, file=sys.stderr)
        raise SystemExit(1)
