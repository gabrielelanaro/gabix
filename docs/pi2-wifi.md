# Raspberry Pi 2B Wi-Fi

The managed Wi-Fi kernel supports the tested USB Realtek `0bda:8176`
RTL8188CU adapter on a 2.4 GHz WPA2-Personal network using CCMP.
The current station transmits at 1 Mbps for a conservative initial radio path.
WPA3, required protected management frames, 5 GHz, roaming, and USB hotplug
are outside this implementation.

## Build and credentials

The build reads `~/.config/gabix/pi2-wifi.json`, a private file containing
`ssid` and a 32-byte PBKDF2-derived `pmk_hex`. The derivation uses the Wi-Fi
passphrase and SSID with PBKDF2-HMAC-SHA1, 4096 iterations. The PMK grants
access to this network and must be protected like the password.

The firmware is OpenBSD's `urtwn-rtl8192cfwT` for the detected TSMC chip.
Its tested SHA-256 is
`f7cbc5d298e7df58cd0a7ce143b680edacdf46bf74fc20c1bb851db9e57c46a9`.
Keep the firmware's supplied license alongside a locally obtained copy;
firmware binaries are excluded from this repository.
See [primary references](pi2-wifi-references.md) for driver and firmware provenance.

```sh
GABIX_WIFI_FIRMWARE=/absolute/path/to/urtwn-rtl8192cfwT \
GABIX_ARM_PROFILE=release sh scripts/build-arm.sh pi2-wifi-managed
```

The image, compiler output, and PMK remain in ignored `target/private-wifi/`.
This image embeds network and management credentials; keep it private.
The existing management key is read from `~/.config/gabix/pi2-management.key`.

## Connection and diagnostics

`targets/armv7/src/raspi2b/wifi_ethernet.rs` advances scanning,
authentication, association, and WPA2 on each poll. It then exposes Ethernet
frames through the existing `FrameDevice` interface. DHCP, UDP echo, SD
uploads, and authenticated management use that interface.

After 15 seconds without a beacon or accepted data, the station clears its
connection and retries. Authentication and association have bounded retries;
a failed security exchange starts a fresh connection. Each new connection
uses a fresh hardware-generated nonce. The foreground service refreshes a
15-second watchdog so a stalled driver can recover through a reboot.

Persistent logs include Wi-Fi state changes, rejection reasons, DHCP events,
and boot/update decisions. They exclude keys and handshake packets.
The Wi-Fi build selects the supported dongle when present; when absent it
uses the existing Ethernet path. Removing a dongle during operation requires
a reboot; simultaneous Ethernet and Wi-Fi routing is not implemented.

```sh
python3 scripts/pi2-manage.py status --address 192.168.178.128
python3 scripts/pi2-manage.py logs --address 192.168.178.128
```

The router assigns the address through DHCP, so check its device list if the
address changes. The adapter MAC is `00:0b:81:95:0f:dc`.

## Validation

The explicit physical fixture `tests/test_pi2_wifi.py` uploads a RAM kernel,
requires association and DHCP, deliberately disconnects, and requires a fresh
handshake and second lease. Host tests cover malformed frames, replay packet
numbers, AES key unwrapping, handshake integrity, duplicate messages, and
group-key replacement. Group-key replacement has a host vector; an actual
router-triggered group rekey remains a separate hardware check.

Raw captures belong in ignored `target/runs/`. Summary results belong here
or in `LEARNING.md`. Keep the UART loader and previous SD kernel available
while validating a new build.

The installed image was uploaded over Wi-Fi and read back exactly (236,236
bytes, CRC32 `98195a23`, 18.51 seconds). Its authenticated reboot selected
SD copy B and confirmed startup; DHCP, management, previous-boot log reads,
and cross-boot replay rejection passed. Copy A retains the prior Ethernet
kernel. QEMU Ethernet UDP echo also passed after the USB transport changes.

## Finding the Pi by name

The managed ARM service implements IPv4 mDNS host lookup as `gabix-pi2.local`.
On a Mac connected to the same local network, use:

```sh
python3 scripts/pi2-manage.py status --address gabix-pi2.local
python3 scripts/pi2-manage.py logs --address gabix-pi2.local
```

DHCP still assigns the address. The Pi answers local multicast questions with
that address, so the caller can resolve the name again after a lease changes.
The network must permit multicast between clients; guest-network isolation can
prevent discovery. This provides host-name lookup, not a web page or DNS-SD
service browser. IPv6 and reverse-address lookup are not implemented.

The implementation in `crates/kernel-core/src/net/mdns.rs` sends three probes
250 ms apart before announcing its address. A name conflict selects a suffix
such as `gabix-pi2-0002.local` and starts probing again. Incoming DNS names use
bounded compression-pointer parsing. Queries support IPv4 A and ANY records,
known-answer suppression, requested unicast replies and legacy DNS clients.
The packet handling follows [RFC 6762](https://www.rfc-editor.org/rfc/rfc6762).
The ARM network session invokes it alongside DHCP; x86 does not yet register a
host with this responder.

Run `python3 tests/test_pi2_mdns.py` on the local network for a fresh multicast
lookup, native name resolution and UDP echo by name. Routine output belongs
in `target/runs/`. A software reboot keeps USB power present; power-removal
startup is a separate physical check.

Physical validation on 2026-11-16: build CRC32 `fedce5c8` (243,968 bytes)
was written/read back over Wi-Fi and confirmed on boot 16. Fresh multicast
lookup, native macOS resolution, UDP echo and authenticated management by name
pass after reboot with UART/Ethernet disconnected. The test selects the local
interface toward the host because a VPN may own the default multicast route;
`--interface-address` can explicitly select the Mac's Wi-Fi IPv4 address.
