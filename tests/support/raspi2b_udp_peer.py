#!/usr/bin/env python3

import socket
import struct
import sys
from pathlib import Path

from arm_udp_peer import HOST_MAC, receive_frame, udp_frame, verify_reply


def run(socket_path: Path) -> None:
    socket_path.unlink(missing_ok=True)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(socket_path))
        listener.listen(1)
        with listener.accept()[0] as connection:
            receive_frame(connection)
            request = udp_frame()
            connection.sendall(struct.pack("!I", len(request)) + request)
            while True:
                reply = receive_frame(connection)
                if reply[:6] == HOST_MAC and reply[12:14] == b"\x08\x00":
                    verify_reply(reply)
                    break


if __name__ == "__main__":
    run(Path(sys.argv[1]))
