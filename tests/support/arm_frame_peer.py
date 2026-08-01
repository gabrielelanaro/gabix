#!/usr/bin/env python3

import socket
import struct
import sys
from pathlib import Path


def receive_exact(connection: socket.socket, length: int) -> bytes:
    data = b""
    while len(data) < length:
        chunk = connection.recv(length - len(data))
        if not chunk:
            raise EOFError("QEMU disconnected")
        data += chunk
    return data


def run(socket_path: Path) -> None:
    socket_path.unlink(missing_ok=True)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(socket_path))
        listener.listen(1)
        with listener.accept()[0] as connection:
            length = struct.unpack("!I", receive_exact(connection, 4))[0]
            frame = receive_exact(connection, length)
            if (
                frame[:6] != b"\xff" * 6
                or frame[12:14] != b"\x88\xb5"
                or frame[14:29] != b"gabix-arm-frame"
            ):
                raise RuntimeError(f"unexpected Ethernet frame: {frame[:32].hex()}")
            print("[peer] ARM Ethernet frame received")


if __name__ == "__main__":
    run(Path(sys.argv[1]))
