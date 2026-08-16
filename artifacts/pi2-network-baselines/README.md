# Raspberry Pi 2 physical Ethernet baselines

Each stage changes one hardware boundary and remains immutable after its
physical result is recorded. Development payloads are loaded into RAM by the
SD-resident UART loader; these tests do not write the SD card.

| Stage | Artifact | Bytes | SHA-256 | Physical result |
|---|---|---:|---|---|
| NET8 | `net8-usb-enumeration.img` | 7,386 | `3c618e58772a044d149665960874578ab17031e90eb49001b610543cbe88eaab` | DWC2 initialized; root device `0424:9514` class `09` enumerated as the onboard SMSC hub; child `0424:EC00` class `FF` enumerated and recognized as LAN9512; watchdog returned to `GABIX LOADER READY` |

Exact UART output is preserved in `net8-usb-enumeration-physical.log`.
