#!/usr/bin/env python3
"""Upload one userspace ELF to a running Gabix Pi 2 development kernel."""

import argparse
import importlib.util
import os
import sys
from pathlib import Path


MAX_USER_ELF_SIZE = 512 * 1024
USER_READY = b"GABIX USER READY"
USER_UPLOAD_OK = b"USER UPLOAD OK"
USER_UPLOAD_FAILED = b"USER UPLOAD FAILED"


def load_transport():
    path = Path(__file__).with_name("upload-pi2-kernel.py")
    spec = importlib.util.spec_from_file_location("pi2_uart_transport", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("uart", help="serial device, for example /dev/cu.usbserial-0001")
    parser.add_argument("user_elf", type=Path)
    parser.add_argument("--ready-timeout", type=float, default=30.0)
    parser.add_argument("--result-timeout", type=float, default=10.0)
    parser.add_argument("--exit-timeout", type=float, default=30.0)
    parser.add_argument("--chunk-size", type=int, default=4096)
    parser.add_argument("--inter-byte-delay-us", type=int, default=500)
    parser.add_argument("--retries", type=int, default=3)
    parser.add_argument("--wait-ready", action="store_true")
    parser.add_argument("--wait-for-exit", action="store_true")
    args = parser.parse_args()

    transport = load_transport()
    transport.validate_transfer_options(
        args.chunk_size, args.retries, args.inter_byte_delay_us
    )
    image = args.user_elf.read_bytes()
    if not image or len(image) > MAX_USER_ELF_SIZE:
        raise ValueError(f"user ELF size must be 1..{MAX_USER_ELF_SIZE} bytes")
    upload = transport.build_upload(image)

    fd = transport.open_uart(args.uart)
    try:
        if args.wait_ready:
            print(f"waiting for user loader on {args.uart}...", file=sys.stderr)
            transport.wait_for(fd, USER_READY, args.ready_timeout)
        for attempt in range(1, args.retries + 1):
            print(
                f"sending {len(image)}-byte user ELF "
                f"(attempt {attempt}/{args.retries})",
                file=sys.stderr,
            )
            transport.write_all(
                fd, upload, args.chunk_size, args.inter_byte_delay_us
            )
            accepted, result = transport.wait_for_result(
                fd,
                args.result_timeout,
                success_marker=USER_UPLOAD_OK,
                failure_marker=USER_UPLOAD_FAILED,
            )
            if accepted:
                if args.wait_for_exit and not transport.wait_for_marker_after(
                    fd,
                    result,
                    after_marker=USER_UPLOAD_OK,
                    marker=USER_READY,
                    timeout=args.exit_timeout,
                ):
                    raise TimeoutError("timed out waiting for userspace exit")
                return 0
            print("kernel rejected the user ELF; retrying", file=sys.stderr)
        return 1
    finally:
        os.close(fd)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, TimeoutError, ValueError) as error:
        print(f"user upload failed: {error}", file=sys.stderr)
        raise SystemExit(1)
