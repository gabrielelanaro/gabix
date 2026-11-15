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
