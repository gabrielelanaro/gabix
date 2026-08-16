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
