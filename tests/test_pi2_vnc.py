#!/usr/bin/env python3
"""Upload the VNC ELF and check framebuffer requests on a physical Pi."""
import argparse
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import unittest

from test_pi2_wifi_tcp_echo import ROOT, client


class Peer:
    def __init__(self, sock, reader):
        self.write = sock.sendall
        self.read = reader.read


DESKTOP = 512
BG, GOLD = bytes.fromhex("96421900"), bytes.fromhex("28cdff00")


def expected(x, y, colors=(BG, GOLD)):
    col, row = x // 8, y // 8
    return colors[8 <= row < 56 and 2 * abs(2*col + 1 - 64) < 2*row + 1 - 16]


def request(peer, incremental=False, rect=(0, 0, DESKTOP, DESKTOP)):
    peer.write(struct.pack(">BB4H", 3, incremental, *rect))


def check_scene(peer):
    peer.write(struct.pack(">BBHi", 2, 0, 1, 2))  # advertise RRE
    request(peer)
    read_scene(peer)


def read_scene(peer, dx=0, dy=0):
    assert peer.read(16) == struct.pack(">BBH4Hi", 0, 0, 1, 0, 0, DESKTOP, DESKTOP, 2)
    count, = struct.unpack(">I", peer.read(4))
    assert count <= 4096
    actual = bytearray(peer.read(4) * DESKTOP**2)
    for _ in range(count):
        color = peer.read(4)
        x, y, w, h = struct.unpack(">4H", peer.read(8))
        assert x+w <= DESKTOP and y+h <= DESKTOP
        for row in range(y,y+h):
            off = 4*(row*DESKTOP+x)
            actual[off:off+4*w] = color*w
    assert actual == b"".join(expected(x-dx,y-dy) for y in range(DESKTOP) for x in range(DESKTOP))


def check_hover(peer):
    previous = None
    for x, y in [(30, 30), (256, 320)]:
        request(peer, incremental=True)
        peer.write(struct.pack(">BBHH", 5, 0, x, y))  # no button pressed
        regions = ([previous] if previous else []) + [(x,y)]
        assert peer.read(4) == struct.pack(">BBH", 0, 0, len(regions))
        for cx,cy in regions:
            assert peer.read(12) == struct.pack(">4Hi", cx-5, cy-5, 11, 11, 0)
            for py in range(cy-5,cy+6):
                for px in range(cx-5,cx+6):
                    d = (px-x)**2 + (py-y)**2
                    color = bytes.fromhex("ffffff00") if d <= 9 else bytes(4) if d <= 25 else expected(px,py)
                    assert peer.read(4) == color, (px,py)
        previous = (x,y)


def check_drag(peer):
    def pointer(button, x, y):
        peer.write(struct.pack(">BBHH", 5, button, x, y))

    # Restore the native format and keep the viewer-local cursor enabled.
    peer.write(bytes.fromhex("00000000 2018000100ff00ff00ff100800000000"))
    peer.write(struct.pack(">BBHii", 2, 0, 2, 2, -239))
    request(peer)
    assert peer.read(16) == struct.pack(">BBH4Hi", 0, 0, 2, 5, 5, 11, 11, -239)
    peer.read(121*4+22)
    # The update header and cursor rectangle are already consumed.
    assert peer.read(12) == struct.pack(">4Hi", 0, 0, DESKTOP, DESKTOP, 2)
    runs, = struct.unpack(">I", peer.read(4))
    peer.read(4 + runs*12)

    pointer(1, 20, 20)  # Pressing the background and crossing gold cannot grab.
    pointer(1, 240, 320)
    pointer(0, 240, 320)
    request(peer)
    read_scene(peer)

    request(peer, incremental=True)  # The update can precede the motion.
    pointer(1, 240, 320)
    pointer(1, 280, 296)
    read_scene(peer, 40, -24)
    request(peer, incremental=True)
    pointer(1, 320, 272)
    read_scene(peer, 80, -48)  # Crosses the right clipping edge.
    pointer(0, 312, 280)  # Final motion delivered with button release.
    pointer(0, 32, 32)
    request(peer)
    read_scene(peer, 72, -40)

    pointer(1, 312, 280)  # Grab again from the translated frame.
    pointer(0, 240, 320)
    request(peer)
    read_scene(peer)


def check_board(address):
    with tempfile.TemporaryDirectory() as tmp:
        elf = Path(tmp) / "arm_vnc.elf"
        subprocess.run(["sh", "scripts/build-arm-user.sh", "tests/fixtures/arm_vnc.rs", elf],
                       check=True, cwd=ROOT)
        pi = client.management.Pi(address)
        try:
            before = pi.status()
            shapes = Path(tmp) / "shapes.elf"
            subprocess.run(["sh", "scripts/build-arm-user.sh", "tests/fixtures/arm_v3d_shapes.rs", shapes],
                           cwd=ROOT, check=True)
            assert client.wait_for_exit(pi, client.start(pi, shapes)) == 0
            session = client.start(pi, elf)
            with socket.create_connection((address, 5900), timeout=20) as sock:
                with sock.makefile("rb") as reader:
                    rfb = Peer(sock, reader)
                    version = b"RFB 003.008\n"
                    assert rfb.read(12) == version
                    rfb.write(version)
                    assert rfb.read(2) == b"\x01\x01"
                    rfb.write(b"\x01")
                    assert rfb.read(4) == bytes(4)
                    rfb.write(b"\x01")
                    assert rfb.read(4) == struct.pack(">HH", DESKTOP, DESKTOP)
                    assert rfb.read(16) == bytes.fromhex("2018000100ff00ff00ff100800000000")
                    assert rfb.read(4) == struct.pack(">I", len(b"Gabix V3D triangle"))
                    assert rfb.read(18) == b"Gabix V3D triangle"
                    check_scene(rfb)
                    check_hover(rfb)
                    rfb.write(struct.pack(">BBH", 2, 0, 0))  # Raw only
                    rfb.write(bytes.fromhex("00000000 10100101 001f003f001f 0b0500 000000"))
                    rect = (128, 232, 64, 16)
                    request(rfb, rect=rect)
                    assert rfb.read(16) == struct.pack(">BBH4Hi", 0, 0, 1, *rect, 0)
                    colors = (bytes.fromhex("1a12"), bytes.fromhex("fe65"))
                    assert rfb.read(64*16*2) == b"".join(expected(x,y,colors)
                        for y in range(232,248) for x in range(128,192))
                    rfb.write(struct.pack(">BBHi", 2, 0, 1, -239))
                    request(rfb, rect=(0, 0, 1, 1))
                    assert rfb.read(16) == struct.pack(">BBH4Hi", 0, 0, 2, 5, 5, 11, 11, -239)
                    sprite = rfb.read(11*11*2)
                    mask = rfb.read(22)
                    for y in range(11):
                        for x in range(11):
                            d = (x-5)**2 + (y-5)**2
                            assert bool(mask[2*y+x//8] & (128 >> (x%8))) == (d <= 25)
                            offset = 2*(11*y+x)
                            assert sprite[offset:offset+2] == (b"\xff\xff" if d <= 9 else bytes(2))
                    assert rfb.read(14) == struct.pack(">4Hi", 0, 0, 1, 1, 0) + colors[0]
                    check_drag(rfb)
                    sock.shutdown(socket.SHUT_WR)
                    assert rfb.read(1) == b""
            assert client.wait_for_exit(pi, session) == 0
            after = pi.status()
            assert (before["boot"], before["kernel_crc32"]) == (after["boot"], after["kernel_crc32"])
            print(f"Boot {after['boot']}: 512x512 frame, two hover positions, cursor restoration, RGB565 crop and drag frames matched")
        finally:
            pi.close()


@unittest.skipUnless(os.getenv("GABIX_PI_TEST_ADDRESS"), "physical Pi test")
def test_request_displays_v3d_frame():
    check_board(os.environ["GABIX_PI_TEST_ADDRESS"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--address", default="gabix-pi2.local")
    check_board(parser.parse_args().address)
