#!/usr/bin/env python3

import socket
import struct
import sys
from pathlib import Path

GUEST_MAC = bytes.fromhex("525400123456")
SERVER_MAC = bytes.fromhex("525400123499")
ZERO_IP = bytes(4)
BROADCAST_IP = bytes([255, 255, 255, 255])
SERVER_IP = bytes([10, 0, 2, 2])
LEASE_IP = bytes([10, 0, 2, 15])


def receive_exact(connection: socket.socket, length: int) -> bytes:
    data = bytearray()
    while len(data) < length:
        chunk = connection.recv(length - len(data))
        if not chunk:
            raise ConnectionError("QEMU disconnected during DHCP exchange")
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


def dhcp_message_type(payload: bytes) -> int | None:
    if len(payload) < 241 or payload[236:240] != bytes([99, 130, 83, 99]):
        return None
    offset = 240
    while offset < len(payload):
        code = payload[offset]
        offset += 1
        if code == 255:
            return None
        if code == 0:
            continue
        if offset >= len(payload):
            return None
        length = payload[offset]
        offset += 1
        value = payload[offset : offset + length]
        offset += length
        if code == 53 and length == 1:
            return value[0]
    return None


def receive_dhcp(connection: socket.socket, expected_type: int) -> bytes:
    while True:
        frame = receive_frame(connection)
        if len(frame) < 42 or frame[12:14] != b"\x08\x00" or frame[23] != 17:
            continue
        ip_length = struct.unpack("!H", frame[16:18])[0]
        udp = frame[34 : 14 + ip_length]
        if len(udp) < 8 or udp[:4] != struct.pack("!HH", 68, 67):
            continue
        payload = udp[8:]
        if dhcp_message_type(payload) == expected_type:
            return payload


def reply_payload(request: bytes, message_type: int) -> bytes:
    payload = bytearray(300)
    payload[0:4] = bytes([2, 1, 6, 0])
    payload[4:8] = request[4:8]
    payload[16:20] = LEASE_IP
    payload[20:24] = SERVER_IP
    payload[28:34] = request[28:34]
    payload[236:240] = bytes([99, 130, 83, 99])
    options = bytearray()
    for code, value in (
        (53, bytes([message_type])),
        (54, SERVER_IP),
        (1, bytes([255, 255, 255, 0])),
        (3, SERVER_IP),
        (6, SERVER_IP),
        (51, struct.pack("!I", 3600)),
    ):
        options += bytes([code, len(value)]) + value
    options += b"\xff"
    payload[240 : 240 + len(options)] = options
    return bytes(payload[: 240 + len(options)])


def udp_frame(payload: bytes) -> bytes:
    udp_length = 8 + len(payload)
    udp = bytearray(struct.pack("!HHHH", 67, 68, udp_length, 0) + payload)
    pseudo = SERVER_IP + BROADCAST_IP + bytes([0, 17]) + struct.pack("!H", udp_length)
    udp[6:8] = struct.pack("!H", checksum(pseudo + udp))
    ip_length = 20 + udp_length
    ip = bytearray(
        struct.pack(
            "!BBHHHBBH4s4s",
            0x45,
            0,
            ip_length,
            1,
            0x4000,
            64,
            17,
            0,
            SERVER_IP,
            BROADCAST_IP,
        )
    )
    ip[10:12] = struct.pack("!H", checksum(ip))
    return bytes([255] * 6) + SERVER_MAC + b"\x08\x00" + ip + udp


def send_frame(connection: socket.socket, frame: bytes) -> None:
    connection.sendall(struct.pack("!I", len(frame)) + frame)


def run(socket_path: Path) -> None:
    socket_path.unlink(missing_ok=True)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(socket_path))
        listener.listen(1)
        with listener.accept()[0] as connection:
            discover = receive_dhcp(connection, 1)
            send_frame(connection, udp_frame(reply_payload(discover, 2)))
            request = receive_dhcp(connection, 3)
            send_frame(connection, udp_frame(reply_payload(request, 5)))
            while connection.recv(4096):
                pass


if __name__ == "__main__":
    run(Path(sys.argv[1]))
