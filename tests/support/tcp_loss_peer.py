#!/usr/bin/env python3

import socket
import struct
import sys
from pathlib import Path

SERVER_MAC = bytes.fromhex("525400123499")
SERVER_IP = bytes((10, 0, 2, 2))
SERVER_PORT = 8080
SERVER_ISN = 0x10203040


def checksum(data: bytes) -> int:
    if len(data) % 2:
        data += b"\0"
    total = sum(struct.unpack(f"!{len(data) // 2}H", data))
    while total >> 16:
        total = (total & 0xFFFF) + (total >> 16)
    return (~total) & 0xFFFF


def send_frame(connection: socket.socket, frame: bytes) -> None:
    connection.sendall(struct.pack("!I", len(frame)) + frame)


def receive_exact(connection: socket.socket, length: int) -> bytes:
    data = b""
    while len(data) < length:
        chunk = connection.recv(length - len(data))
        if not chunk:
            raise EOFError("QEMU disconnected")
        data += chunk
    return data


def receive_frame(connection: socket.socket) -> bytes:
    length = struct.unpack("!I", receive_exact(connection, 4))[0]
    if length > 65535:
        raise RuntimeError(f"invalid Ethernet frame length {length}")
    return receive_exact(connection, length)


def arp_reply(frame: bytes) -> bytes:
    guest_mac = frame[6:12]
    arp = frame[14:42]
    guest_ip = arp[14:18]
    return (
        guest_mac
        + SERVER_MAC
        + b"\x08\x06"
        + struct.pack("!HHBBH", 1, 0x0800, 6, 4, 2)
        + SERVER_MAC
        + SERVER_IP
        + guest_mac
        + guest_ip
    )


def tcp_frame(
    guest_mac: bytes,
    guest_ip: bytes,
    guest_port: int,
    sequence: int,
    acknowledgment: int,
    flags: int,
    payload: bytes = b"",
) -> bytes:
    tcp = struct.pack(
        "!HHIIBBHHH",
        SERVER_PORT,
        guest_port,
        sequence,
        acknowledgment,
        5 << 4,
        flags,
        4096,
        0,
        0,
    ) + payload
    pseudo = SERVER_IP + guest_ip + b"\0\x06" + struct.pack("!H", len(tcp))
    tcp_checksum = checksum(pseudo + tcp)
    tcp = tcp[:16] + struct.pack("!H", tcp_checksum) + tcp[18:]

    total_length = 20 + len(tcp)
    ip = struct.pack(
        "!BBHHHBBH4s4s",
        0x45,
        0,
        total_length,
        1,
        0,
        64,
        6,
        0,
        SERVER_IP,
        guest_ip,
    )
    ip = ip[:10] + struct.pack("!H", checksum(ip)) + ip[12:]
    return guest_mac + SERVER_MAC + b"\x08\x00" + ip + tcp


def run(socket_path: Path) -> None:
    socket_path.unlink(missing_ok=True)
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(socket_path))
        listener.listen(1)
        with listener.accept()[0] as connection:
            guest_mac = None
            guest_ip = None
            guest_port = None
            dropped = None

            while True:
                frame = receive_frame(connection)
                if len(frame) < 14:
                    continue
                ethertype = frame[12:14]

                if ethertype == b"\x08\x06" and len(frame) >= 42:
                    arp = frame[14:42]
                    if arp[6:8] == b"\x00\x01" and arp[24:28] == SERVER_IP:
                        guest_mac = frame[6:12]
                        guest_ip = arp[14:18]
                        send_frame(connection, arp_reply(frame))
                    continue

                if ethertype != b"\x08\x00" or len(frame) < 54:
                    continue
                ip = frame[14:]
                header_length = (ip[0] & 0x0F) * 4
                total_length = struct.unpack("!H", ip[2:4])[0]
                if ip[9] != 6 or ip[16:20] != SERVER_IP:
                    continue
                tcp = ip[header_length:total_length]
                guest_port, destination, sequence, acknowledgment = struct.unpack(
                    "!HHII", tcp[:12]
                )
                if destination != SERVER_PORT:
                    continue
                flags = tcp[13]
                tcp_header_length = (tcp[12] >> 4) * 4
                payload = tcp[tcp_header_length:]
                guest_mac = frame[6:12]
                guest_ip = ip[12:16]

                if flags & 0x02:
                    send_frame(
                        connection,
                        tcp_frame(
                            guest_mac,
                            guest_ip,
                            guest_port,
                            SERVER_ISN,
                            sequence + 1,
                            0x12,
                        ),
                    )
                    continue

                if not payload:
                    continue

                identity = (sequence, payload)
                if dropped is None:
                    dropped = identity
                    continue
                if identity != dropped:
                    raise RuntimeError("guest sent different data before retransmitting")

                next_guest_sequence = sequence + len(payload)
                send_frame(
                    connection,
                    tcp_frame(
                        guest_mac,
                        guest_ip,
                        guest_port,
                        SERVER_ISN + 1,
                        next_guest_sequence,
                        0x10,
                    ),
                )
                send_frame(
                    connection,
                    tcp_frame(
                        guest_mac,
                        guest_ip,
                        guest_port,
                        SERVER_ISN + 1,
                        next_guest_sequence,
                        0x18,
                        b"!",
                    ),
                )
                print("[peer] retransmission observed", flush=True)
                return


if __name__ == "__main__":
    run(Path(sys.argv[1]))
