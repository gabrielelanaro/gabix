# Raspberry Pi 2 physical Ethernet baselines

Each stage changes one hardware boundary and remains immutable after its
physical result is recorded. Development payloads are loaded into RAM by the
SD-resident UART loader; these tests do not write the SD card.

| Stage | Artifact | Bytes | SHA-256 | Physical result |
|---|---|---:|---|---|
| NET8 | `net8-usb-enumeration.img` | 7,386 | `3c618e58772a044d149665960874578ab17031e90eb49001b610543cbe88eaab` | DWC2 initialized; root device `0424:9514` class `09` enumerated as the onboard SMSC hub; child `0424:EC00` class `FF` enumerated and recognized as LAN9512; watchdog returned to `GABIX LOADER READY` |
| NET9 | `net9-lan9512-link.img` | 12,881 | `576e39e594b1dc97ec25b08be77ac7e41dd66163e339690e6e1bded2ba78f11c` | LAN9512 configured and printed `NET9 LAN9512 CONFIG OK`; Mac interface `en6` became active at 100baseTX full-duplex with flow control for the probe lifetime |
| NET10 | `net10-lan9512-raw-tx.img` | 17,905 | `94102ea52f445c54db1d30538d4770ea8b78fea2ea4c0c2a58eff7f554524896` | After a five-second link wait, all 16 USB bulk transfers completed and Mac `en6` increased by exactly 16 packets and 960 bytes, matching sixteen 60-byte broadcast frames |
| NET11 | `net11-lan9512-raw-rx.img` | 19,105 | `2f5145652139fa7672b7d4db763c02a4b0c666a8b1838448236d575f75136a28` | Received a 110-byte IPv6 frame from Mac `en6` MAC `E8:EA:6A:2D:FE:C4`, addressed to multicast MAC `33:33:00:00:00:16`, and printed it over UART |
| NET12 | `net12-lan9512-dhcp-size-tx.img` | 17,809 | `c5bef73ef759d84929659effb5c709d59944062b7b96af42fba6058dc1947743` | Changed only the NET10 frame length from 60 to Gabix's 293-byte DHCP-discover frame length; all 16 transfers completed and Mac `en6` increased by exactly 16 packets and 4,688 bytes |
| NET13 | `net13-lan9512-dhcp-packet-tx.img` | 19,705 | `8f70f34e0d5aa258e34d7bdec1d5afff5c724b55f63537c7c961c970e0855326` | Used the real Gabix DHCP client and IPv4/UDP/Ethernet packet builders in the minimal passing probe; all 16 discovers completed and Mac `en6` increased by exactly 16 packets and 4,688 bytes |
| NET14 | `net14-full-pre-timer-release-dhcp-tx.img` | 44,400 | `4042fce909afc788c65c41f1b0f92b8ad8cd8df7249d702fc280a5a58b39c716` | Full Gabix boot path through heap and SD inspection, with DHCP sent before timer initialization; release build sent all 16 frames and Mac `en6` increased by exactly 16 packets and 4,688 bytes, while the corresponding debug build sent zero wire packets |
| NET15 | `net15-full-release-dhcp.img` | 47,100 | `2fe0b857cbb358598506b18e0cc0a93b768bf290d479c5919c6093a698ccdbb2` | Normal integrated release kernel with timer and receive polling completed DHCP against the controlled Mac peer, printed `dhcp configured`, and returned to the UART loader through the watchdog |
| NET16 | `net16-full-release-udp-echo.img` | 47,100 | `2fe0b857cbb358598506b18e0cc0a93b768bf290d479c5919c6093a698ccdbb2` | With a temporary static Mac ARP entry for the Pi, the NET15 kernel echoed `GABIX-PI2-UDP-NET16` byte-for-byte from UDP port 5555 at `10.0.2.15`; this physically verifies integrated IPv4/UDP RX and TX while leaving dynamic ARP as a separate stage |
| NET17 | `net17-lan9512-repeated-rx.img` | 19,565 | `6996851b3f7069fa44525c04c2094e62017f59b3714556f414aa3f330ac3ec2b` | Minimal receive probe consumed 16 successive frames from a controlled Mac UDP stream and printed the sixteenth 60-byte IPv4 frame, proving repeated LAN9512 bulk-IN and data-toggle handling |

Exact UART output is preserved in the corresponding `*-physical.log` files.
NET9's Mac-side link transition is preserved in
`net9-lan9512-link-mac-poll.log`. Its host uploader timed out at the same
15-second boundary used by the probe watchdog; the UART success marker and
Mac link-up occurred before that host-side timeout.
NET10's UART result and Mac interface-counter transition are preserved in
`net10-lan9512-raw-tx-physical.log` and
`net10-lan9512-raw-tx-mac-counters.log`.
The preserved NET10 image was rerun unchanged on 2026-08-16 before the DHCP
bring-up investigation. Its SHA-256 matched the table above, UART again
reported 16 successful transfers, and Mac `en6` again increased by exactly 16
packets and 960 bytes. The transcript is preserved in
`net10-lan9512-raw-tx-rerun-2026-08-16.log`.
NET11's decoded receive result is preserved in
`net11-lan9512-raw-rx-physical.log`.
NET12's UART result is preserved in
`net12-lan9512-dhcp-size-tx-physical.log`.
NET13's UART result is preserved in
`net13-lan9512-dhcp-packet-tx-physical.log`.
NET14's UART result is preserved in
`net14-full-pre-timer-release-dhcp-tx-physical.log`.
NET15's full DHCP exchange is preserved in
`net15-full-release-dhcp-physical.log`.
NET16's UART trace and Mac peer result are preserved in
`net16-full-release-udp-echo-physical.log` and
`net16-full-release-udp-echo-peer.log`.
NET17's UART trace is preserved in
`net17-lan9512-repeated-rx-physical.log`.

## 2026-11-12 — Receive recovery and persistent server

- `net18-before-fix-physical.log`: DHCP completed, but dynamic ARP, ping, and UDP failed in a synchronized test before the watchdog reset.
- `net18-usb-status-physical.log`: after removing the forced DATA0 reset, ARP and one ping worked. The first idle receive had `HCINT=0x10` (NAK), `HCCHAR=0x80888a00` (enabled), and `HCTSIZ=0x401805f2`. The driver returned without halting the DMA channel.
- `net18-idle-echo-physical.log`: with explicit channel halt on NAK, both pings and all three UDP messages passed, including one-second idle periods. The watchdog returned the diagnostic build to the loader.
- `net19-uart-reboot-physical.log`: the persistent `pi2-dev` server accepted `arm_reboot.elf` through its UART user-program loader and returned to `GABIX LOADER READY`. A subsequent kernel upload restored the echo service.

Reproduce the network regression from the loader with
`python3 scripts/check-pi2-network.py`. See the runbook for the release build
and Mac DHCP peer prerequisites. Use `scripts/pi2-echo.py` for a message to an
already-running persistent server.

The halt operation follows the DMA abort mechanism described in Linux's
[`dwc2_hc_halt`](https://code.googlesource.com/linux/torvalds/linux/+/586b5dfb51b962c1b6c06495715e4c4f76a7fc5a/drivers/usb/dwc2/hcd.c).
A hardware regression that sends traffic after idle would have caught this
failure: boot-only and immediate-response tests did not exercise reuse of an
active NAKed channel.
