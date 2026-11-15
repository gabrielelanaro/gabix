# Moving the Pi to the router

The managed kernel boots from SD, obtains an address through DHCP, and accepts
authenticated status, log, update, and reboot commands on UDP port 5556.
Both installed kernel copies support authenticated management.

## Move the cable

1. Stop the temporary Mac DHCP server before reusing its adapter on another LAN:
   `sudo sh scripts/pi2-dhcp-peer-macos.sh stop en6`.
2. Disconnect the Ethernet cable from the Mac adapter. Leave it disconnected for
   at least three seconds so the Pi's two-second link poll observes the change.
3. Connect that cable to a LAN port on the FRITZ!Box. The Pi can stay powered.
4. Look in the router's device list for hostname `gabix-pi2` or MAC
   `02:47:41:42:49:58`, and note its assigned IPv4 address.
5. From a Mac on the same home LAN, use that address:

```sh
python3 scripts/pi2-manage.py status --address 192.168.178.42
python3 scripts/pi2-manage.py logs --address 192.168.178.42
```

The address above is an example. Use the router's actual assignment. Allow about
two minutes after a full reboot: current startup includes isolation tests and a
scan of the log area. A cable move with the kernel already running usually
recovers sooner. Guest-network isolation can prevent the Mac reaching the Pi;
use the normal home LAN for this setup.

The actual FRITZ!Box connection remains to be tested after the cable move. The
preparation tests used the connected physical Pi and the Mac's DHCP service.

## Status and logs

Status reports link state, MAC, IP, mask, gateway, DHCP server, DNS, lease lifetime,
DHCP state/attempts/rejections, network transmit failures, management counters,
boot number, kernel copy/CRC, and log health. Rejected DHCP packets can include
duplicate or stale replies; the counter alone does not mean the lease failed.

`targets/armv7/src/diagnostics.rs` stores a CRC-protected ring in a dedicated
**16 MiB area**, sectors **5,015,552–5,048,319**. The original 1 MiB boot metadata
area remains reserved for boot information. The kernel partition still has
47 MiB spare. There is no filesystem pathname on the card for this raw journal.

Console diagnostic lines first enter a 2 KiB RAM queue. Journal sectors hold up
to 468 bytes of text and flush about once per second while the service runs.
Startup stages and boot/update/reboot decisions request immediate flushes.
Records include a persistent boot number, sequence number, and uptime. A record
can split a long line; the text client joins fragments across records.

The ring overwrites the oldest records when full. Status exposes dropped lines,
SD write/read-back failures, and corrupt records discovered during startup.
A crash can lose unflushed messages. A failed SD device cannot reliably preserve
new evidence; the last successfully stored stage and boot-selection record can
still help explain the failure.

```sh
# Read recent history; the default is the latest 128 journal sectors.
python3 scripts/pi2-manage.py logs --address PI_IP --records 128
# Export structured records locally into the ignored run directory.
mkdir -p target/runs/router
python3 scripts/pi2-manage.py logs --address PI_IP --json > target/runs/router/logs.json
```

`reset_status_raw` retains the power-manager register value. We do not infer a
reset cause from an unverified bit interpretation. Boot logs also retain the
active/pending/attempted selection before the kernel confirms its boot, so a
fallback remains observable even when the failed kernel never initialized logs.

## DHCP behavior

`targets/armv7/src/network.rs` owns the lease state machine. Discovery retries
continue indefinitely with a capped retry interval. An unanswered request returns
to discovery. The client renews with its existing address at T1, broadcasts at
T2, and removes the address if its lease expires. A valid server rejection or
observed cable change restarts discovery. Diagnostics name the restart reason.

The shared packet codec in `crates/kernel-core/src/net/dhcp.rs` handles renewal
options and server rejections, advertises the hostname/client identifier, and
accepts lists of DNS/router addresses. It uses the first address from each list.
The message rules follow [RFC 2131 sections 4.3.2 and 4.4.5](https://www.rfc-editor.org/rfc/rfc2131.html).

## Updating and rebooting

```sh
GABIX_ARM_PROFILE=release sh scripts/build-arm.sh pi2-managed
python3 scripts/pi2-sd-transfer.py \
  targets/armv7/target/armv7a-none-eabihf/release/kernel-pi2-managed.img \
  --kernel --address PI_IP
python3 scripts/pi2-manage.py reboot --address PI_IP
# After startup:
python3 scripts/pi2-manage.py status --address PI_IP
```

The update preserves the confirmed copy. The SD loader still falls back after
an unconfirmed boot and offers UART recovery if both copies fail. This recovery
loader has no Ethernet implementation; keep the UART adapter available for that
last-resort case.

## Authentication and key storage

The private 32-byte key lives at `~/.config/gabix/pi2-management.key`, with owner
read/write permissions. Build and client commands use it automatically;
`GABIX_PI_MANAGEMENT_KEY` can select another private path. Keep this file across
repository/build cleanup and back it up privately. The key is never committed.
The managed kernel and its recovery image embed the key, so those images are
private artifacts too; new preserved images use owner-only read permissions.

Every management request and reply uses RustCrypto
[HMAC-SHA256](https://docs.rs/hmac/latest/hmac/). A persistent boot number and an
increasing command counter reject replay across ordinary reboots. The most recent
valid duplicate receives its cached reply without executing again. Use one
management client at a time; concurrent clients can contend for that counter.

Only the boot/counter challenge is public. Status, logs, raw reads, updates,
installation, and reboot require authentication. The protocol authenticates
messages without encrypting them. Restoring an older SD image can roll back the
boot counter; rotate the key when restoring if previously captured packets are
a concern. Physical key extraction and malicious kernels are outside this
private-LAN development protocol's protection.

The legacy bootstrap protocol requires explicit `GABIX_PI_LEGACY=1` on the host.
It belongs only on the isolated direct link. The authenticated client never
silently falls back to unsigned requests.

## Validation completed before moving the cable

- Physical SD boot with DHCP, authenticated status, and readable persistent logs.
- A 150-second pause of the Mac DHCP daemon followed by automatic recovery in
  the same boot. Boot 2 acquired its address around 149 seconds; logs retained
  discovery/retry and bound transitions, with zero log-write errors or drops.
- Physical renewal of the 300-second lease at the halfway point, followed by a
  fresh 300-second lease and recorded acknowledgement.
- Unsigned and modified requests rejected; duplicate request handled without
  another command execution.
- Authenticated Ethernet kernel update, Ethernet reboot, address reacquisition,
  previous-boot log retrieval, and old reboot packet rejection after restart.
- Focused host tests for renewal/rebinding/expiry, stale replies, NAKs, unanswered
  requests, and missing link; existing ARM virt QEMU DHCP regression passed.

The controlled outage helper is `scripts/pi2-dhcp-fault-check-macos.sh`. It pauses
only this repository's en6 daemon, requests a network reboot, and resumes DHCP
after 150 seconds, including on interruption. Physical command tests are opt-in
in `tests/test_pi2_management.py`; raw evidence stays under ignored `target/runs/`.

The updated recovery image is registered as `pi2-router-ready-2026-11-15` and
preserved in `~/Documents/Gabix-Images/gabix-pi2-router-ready-2026-11-15.img`.
Its managed kernel was tested on hardware and both image copies were verified
locally; this complete image has not been reflashed as a whole. Earlier protected
images remain available for isolated-link recovery.
