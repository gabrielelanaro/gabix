#!/usr/bin/env python3

import socket
import struct
import sys
from pathlib import Path

GUEST_MAC = bytes.fromhex("525400123456")
HOST_MAC = bytes.fromhex("525400123499")
GUEST_IP = bytes([10, 0, 2, 15])
HOST_IP = bytes([10, 0, 2, 2])
PAYLOAD = b"gabix-arm-udp"


def receive_exact(connection: socket.socket, length: int) -> bytes:
    data = bytearray()
    while len(data) < length:
        chunk = connection.recv(length - len(data))
        if not chunk:
            raise ConnectionError("QEMU disconnected before sending a frame")
        data.extend(chunk)
    return bytes(data)


def receive_frame(connection: socket.socket) -> bytes:
    length = struct.unpack("!I", receive_exact(connection, 4))[0]
    return receive_exact(connection, length)


def checksum(data: bytes) -> int:
    if len(data) % 2:
        data += b"\0"
    total = sum(struct.unpack(f"!{len(data) // 2}H", data))
    while total >> 16:
        total = (total & 0xFFFF) + (total >> 16)
    return (~total) & 0xFFFF


def udp_frame() -> bytes:
    udp_length = 8 + len(PAYLOAD)
    udp = bytearray(struct.pack("!HHHH", 4444, 5555, udp_length, 0) + PAYLOAD)
    pseudo = HOST_IP + GUEST_IP + bytes([0, 17]) + struct.pack("!H", udp_length)
    udp[6:8] = struct.pack("!H", checksum(pseudo + udp))

    ip_length = 20 + udp_length
    ip = bytearray(
        struct.pack("!BBHHHBBH4s4s", 0x45, 0, ip_length, 1, 0x4000, 64, 17, 0, HOST_IP, GUEST_IP)
    )
    ip[10:12] = struct.pack("!H", checksum(ip))
    frame = GUEST_MAC + HOST_MAC + b"\x08\x00" + ip + udp
    return frame.ljust(60, b"\0")


def verify_reply(frame: bytes) -> None:
    if frame[:6] != HOST_MAC or frame[6:12] != GUEST_MAC or frame[12:14] != b"\x08\x00":
        raise RuntimeError(f"unexpected Ethernet reply: {frame[:32].hex()}")
    ip = frame[14:34]
    if checksum(ip) != 0 or ip[12:16] != GUEST_IP or ip[16:20] != HOST_IP or ip[9] != 17:
        raise RuntimeError(f"invalid IPv4 reply: {ip.hex()}")
    ip_length = struct.unpack("!H", ip[2:4])[0]
    udp = frame[34 : 14 + ip_length]
    if udp[:4] != struct.pack("!HH", 5555, 4444) or udp[8:] != PAYLOAD:
        raise RuntimeError(f"unexpected UDP reply: {udp.hex()}")
    pseudo = GUEST_IP + HOST_IP + bytes([0, 17]) + struct.pack("!H", len(udp))
    if checksum(pseudo + udp) != 0:
        raise RuntimeError("invalid UDP checksum")


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
