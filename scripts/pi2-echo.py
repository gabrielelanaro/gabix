#!/usr/bin/env python3
"""Send a message to the running Pi kernel UDP echo service."""

import argparse
import socket
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("message", nargs="?", default="what up buddy how are you")
    parser.add_argument("--address", default="10.0.2.15")
    args = parser.parse_args()
    payload = args.message.encode("utf-8")
    if len(payload) > 1000:
        parser.error("message must fit in 1000 UTF-8 bytes")
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.settimeout(2)
        connection.connect((args.address, 5555))
        print(f"Mac → Pi: {args.message}", flush=True)
        connection.send(payload)
        reply = connection.recv(1500)
        print(f"Pi → Mac: {reply.decode('utf-8', errors='replace')}")
        if reply != payload:
            print("Reply differs from the sent bytes.", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except OSError as error:
        print(f"No echo reply: {error}. Check that the Pi server is running.", file=sys.stderr)
        raise SystemExit(1)
