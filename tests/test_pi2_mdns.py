#!/usr/bin/env python3
"""Explicit physical check: multicast name lookup, native resolution and UDP echo.

Run on the Pi's local network. No SD writes or reboots are performed.
"""
import argparse
import socket
import struct
import time

__test__ = False
NAME = "gabix-pi2.local"
MDNS_GROUP = ("224.0.0.251", 5353)


def test_discovery_resolves_and_echoes(name, interface_address=None):
    encoded = b"".join(bytes([len(label)]) + label.encode("ascii") for label in name.split(".")) + b"\0"
    identifier = 0x4762
    query = struct.pack("!6H", identifier, 0, 1, 0, 0, 0) + encoded + struct.pack("!HH", 1, 1)
    # An ephemeral source port requests a legacy unicast response to a multicast
    # question. This checks the Pi directly, independently of the Mac DNS cache.
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.settimeout(2)
        connection.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 255)
        # VPNs can own the default multicast route. Choose the interface used
        # to reach this host; an explicit address also permits an uncached test.
        if interface_address is None:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as route:
                route.connect((name, 5353))
                interface_address = route.getsockname()[0]
        connection.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_IF,
                              socket.inet_aton(interface_address))
        for attempt in range(5):
            connection.sendto(query, MDNS_GROUP)
            try:
                response, peer = connection.recvfrom(1500)
                break
            except socket.timeout:
                if attempt == 4:
                    raise AssertionError("No mDNS response on the local network")
        expected_header = struct.pack("!6H", identifier, 0x8400, 1, 1, 0, 0)
        expected_question = encoded + struct.pack("!HH", 1, 1)
        expected_record = encoded + struct.pack("!HHIH", 1, 1, 10, 4)
        assert peer[1] == 5353
        assert response == expected_header + expected_question + expected_record + socket.inet_aton(peer[0])
        print(f"PASS: fresh multicast lookup returned {name} = {peer[0]}", flush=True)
    addresses = {entry[4][0] for entry in socket.getaddrinfo(name, 5555, socket.AF_INET, socket.SOCK_DGRAM)}
    assert peer[0] in addresses, (peer, addresses)
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.settimeout(3)
        connection.connect((name, 5555))
        payload = f"mDNS echo {time.monotonic_ns()}".encode()
        connection.send(payload)
        assert connection.recv(1500) == payload
    print("PASS: native name resolution and UDP echo by hostname", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", default=NAME)
    parser.add_argument("--interface-address", help="Local Wi-Fi/Ethernet IPv4 address")
    args = parser.parse_args()
    test_discovery_resolves_and_echoes(args.name, args.interface_address)
