#!/usr/bin/env python3

import socket
import struct
import sys
from pathlib import Path


def receive_exact(connection: socket.socket, length: int) -> bytes:
    data = bytearray()
    while len(data) < length:
        chunk = connection.recv(length - len(data))
        if not chunk:
            raise ConnectionError("QEMU disconnected before sending a frame")
        data.extend(chunk)
    return bytes(data)


def run(socket_path: Path) -> None:
    socket_path.unlink(missing_ok=True)
    frame = (
        bytes.fromhex("525400123456")
        + bytes.fromhex("525400123499")
        + b"\x88\xb5"
        + b"gabix-host-frame"
    )
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(socket_path))
        listener.listen(1)
        with listener.accept()[0] as connection:
            outgoing_length = struct.unpack("!I", receive_exact(connection, 4))[0]
            receive_exact(connection, outgoing_length)
            connection.sendall(struct.pack("!I", len(frame)) + frame)
            while connection.recv(4096):
                pass


if __name__ == "__main__":
    run(Path(sys.argv[1]))
