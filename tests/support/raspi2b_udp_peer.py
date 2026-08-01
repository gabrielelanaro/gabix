#!/usr/bin/env python3

import socket
import struct
import sys
from pathlib import Path

from arm_dhcp_peer import receive_dhcp, reply_payload, send_frame
from arm_dhcp_peer import udp_frame as dhcp_frame
from arm_udp_peer import HOST_MAC, receive_frame, udp_frame, verify_reply


def run(socket_path: Path) -> None:
    socket_path.unlink(missing_ok=True)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(socket_path))
        listener.listen(1)
        with listener.accept()[0] as connection:
            discover = receive_dhcp(connection, 1)
            send_frame(connection, dhcp_frame(reply_payload(discover, 2)))
            request = receive_dhcp(connection, 3)
            send_frame(connection, dhcp_frame(reply_payload(request, 5)))
            request = udp_frame()
            connection.sendall(struct.pack("!I", len(request)) + request)
            while True:
                reply = receive_frame(connection)
                if reply[:6] == HOST_MAC and reply[12:14] == b"\x08\x00":
                    verify_reply(reply)
                    break


if __name__ == "__main__":
    run(Path(sys.argv[1]))
