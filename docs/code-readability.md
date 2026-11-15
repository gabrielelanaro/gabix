# Code readability audit

Goal: a reader can explain each hardware or protocol operation from its call
site, then follow the helper to the exact bytes or register access.

## Rules for this pass

- Name register addresses, meaningful bit fields, protocol tags, units, and
  bounded retry policies. Keep reference-manual names in comments where useful.
- State what an operation changes and why its ordering matters. Define an
  unfamiliar hardware term when it is first used in an execution sequence.
- Make set/clear operations and polling expectations explicit at call sites.
- Preserve behavior, including access widths, ordering, waits, and errors.
- Keep literal calibration data as reference tables. Document provenance and
  interpretation instead of giving every table entry an invented name.
- Mark unknown register semantics honestly. A descriptive-sounding guess is
  worse than an explicit reference-prescribed value.
- Ordinary arithmetic, array indices, and already-clear code do not need
  constants or narration. Avoid changing them just to remove all numerals.

## Audit coverage

Unchecked entries are pending review, not claims that every file needs edits.
There are 141 source files across the three source roots at the start of this
pass, including assembly and bring-up probes. Scripts and behavioral fixtures
will be checked alongside the implementation they exercise.

- [ ] Pi Wi-Fi driver: power, register transport, firmware, packet descriptors,
  calibration, and station transitions
  - [x] `power_on`: named addresses/bits/delays; numbered operation explanations
  - [x] Register polling: explicit `BitState::Set` / `Clear`; bounded USB reads
  - [x] Basic register transport: named vendor-request fields; SETUP/data contract
  - [x] Firmware pages, mailbox ownership, and power-off sequence: named fields and ordering explanations (physical check pending)
  - [x] Station phases, retry timings, and basic management-frame tags
  - [x] Packet descriptor fields, queue/rate selection and USB PID toggling
  - [x] Adapter key-table commands/layout, security enable fields and station-rate registration
  - [x] I/Q calibration result fields, consistency policy and correction selection; reference table contract
  - [x] Factory identity/eFuse decoding and read-completion polling
  - [x] Packet RAM page links, queue reservations, boundaries and page sizes
  - [x] Indirect RF register transport and 20 MHz channel selection
  - [x] Station power groups, retry/contention settings and ACK diagnostic filter
  - [x] Receiver startup sequence, RF interface controls and scan filters
  - [x] Analog preset registers and reference USB-interference sequence
  - [x] Management-frame address ranges shared with the station adapter
  - [x] WPA2 fixed message fields, reply nonce selection and derived key ranges
  - [x] Key-data element fields and RFC 3394 unwrap loop
  - [x] Shared security helper APIs and derivation policy (physical validation pending)
- [ ] Pi USB controller, hub enumeration, Ethernet, and RNDIS
  - [x] LAN9512 register transport, PHY access and SMSC frame headers
  - [x] Ethernet selection and QEMU RNDIS setup/frame fields
  - [x] USB transfer direction/retry API, DMA submission and polling contract
  - [x] USB setup requests, hub-port enumeration, descriptor fields and control stages
  - [x] USB controller initialization and cache/address handling (physical check pending)
- [ ] Pi SD controller, mailbox, timer, reset, and hardware random generator
  - [x] Timer: source rates, interrupt routing/acknowledgement, rollover-safe uptime read
  - [x] Random generator: enable/warm-up, FIFO word count, whole-nonce timeout
  - [x] Mailbox: request layout, power flags, reply fields, RAM/cache ownership
  - [x] Reset/watchdog: protected-write key, countdown units and field limits
  - [x] SD command indices, reset/power fields, sector units and polling contract
  - [x] SD capacity extraction and clock-divider bit packing
- [ ] ARM boot, page tables, interrupts, scheduling, processes, and syscalls
  - [x] Kernel task context switch and scheduler frame creation: named saved-word layout, user-bank offset and IRQ mask
  - [x] ARM memory control bits, table dimensions and range-helper contracts
  - [x] Fixed-process isolation probe ABI and fault-status checks
  - [x] ARM user-entry mode changes and bootstrap ELF-loader contracts
  - [x] ARM syscall frame, dispatch, output bounds and exit paths
  - [x] ARM scheduler task states, process switching and static-stack lifetime
  - [x] ARM exception frame offsets, mode changes and return adjustments
  - [x] Boot GPIO/UART registers and polling console contracts
  - [ ] Memory ownership/cache assumptions
- [ ] ARM loader, SD update/install, authenticated management, and diagnostics
  - [x] Loader/kernel RAM handoff fields and trial-confirmation contracts
  - [x] Loader SD selection, streaming read and UART recovery contracts
- [ ] ARM VirtIO adapters and hardware probes
  - [x] QEMU VirtIO network: queue roles, feature words, packet metadata and buffer reuse
  - [x] QEMU VirtIO block: feature negotiation, descriptor roles, memory ordering and polling contract
  - [x] QEMU virt timer/GIC: named interrupt fields, counter interval and acknowledgement sequence
- [ ] Shared network parsers and state machines, including WPA2
  - [x] Ethernet, ARP, IPv4, ICMP, UDP, and checksum helpers: wire layouts, abbreviations, meaningful field values, and split-buffer checksum explanation
  - [x] DHCP options/header fields and ARM lease retry policy; stack dispatch contracts
  - [x] Beacon/association information elements, capability/rate encoding and EAPOL framing
  - [x] CCMP headers, key/traffic IDs and replay-counter commit order
  - [x] WPA2 message fields, key-data parsing, unwrap and derivation helper contracts
- [ ] Shared ELF, partitions, ext2, memory, loader protocol, and boot records
  - [x] Boot record: field layout, copy-selection state, persistence ownership and MBR bounds
  - [x] UART upload decoder: header fields, incremental storage events, checksum and timeout ownership
  - [x] Shared frame bitmap and both heap wrappers: frame/address units, availability bits, IRQ locking
  - [x] Shared ext2 boot reader: disk fields, inode/group arithmetic, directory records and direct-block limit
  - [x] ARM ELF32 parser: named header offsets, segment/callback contracts
  - [x] x86 ELF64 parser and loader: named fields, page arithmetic and explicit validation/permission limits
  - [x] MBR primary-entry lookup and offset adapter: named layout fields and explicit validation limits
- [ ] x86 boot, paging, context switching, interrupts, PIC/PIT/APIC, and syscalls
  - [x] PIT: named control-word fields and corrected the mode description
  - [x] PIC masks and local APIC enable/address/acknowledgement fields
  - [x] Task context switch and matching new-task/fork stack construction
  - [x] Boot orchestration, GDT/TSS contracts and initial user entry
  - [x] SYSCALL/SYSRET entry, saved argument offsets and STAR selector setup
  - [x] Page-table dimensions, frame arithmetic and clone/copy explanations
  - [x] Interrupt/exception wrapper explanations, stack offsets and fault-bit names (existing exception defects recorded below)
  - [ ] Page-table ownership/synchronization contracts
- [x] x86 PCI and VirtIO drivers (source review; existing QEMU checks pass)
  - [x] PCI bus-0 discovery, BAR fields and configuration access contract
  - [x] VirtIO capability regions and MSI-X table/control parsing
  - [x] Shared x86 VirtIO queue layout, submission/completion contracts and descriptor permissions
  - [x] x86 block initialization, interrupt routing, synchronous read and DMA-buffer contract
  - [x] x86 network initialization, queue/interrupt selection and buffer lifetime
- [ ] x86 scheduling, process lifecycle, file operations, and network stack
  - [x] Syscall user-range/path helpers, file/time handlers and process-entry conventions
  - [x] Socket ABI fields, protocol selection, configuration operations and polling contracts
  - [x] Scheduler state selection, block/wake and lifecycle helper contracts
  - [x] Task creation, fork layout and process-table resource contracts (known limitations below)
  - [x] VFS inode cache, internal directory encoding and descriptor ownership
  - [x] x86 ext2 adapter storage units, pointer roles and read limits
  - [x] x86 ext2 serialized superblock, group, inode and directory field offsets
  - [x] Network-device ownership, receive dispatch and ARP wait contracts
  - [x] UDP queue/waiter ownership, runtime ARP cache and network configuration
  - [x] TCP wire header, SYN MSS option and IPv4 checksum pseudo-header
  - [x] TCP active-open send accounting, ACK removal and retransmission timing
  - [x] TCP state transitions and passive socket ownership
- [ ] Learning snippets and test helpers affected by the edits

## First slice: Wi-Fi power-on

`rtl8188cu.rs` names the constants in `power`, retaining the source-register
aliases. `power_on` shows eight operations in their existing order. The
register helper states that each read is a USB control transfer, so the
poll interval is additional delay and is not the total time per attempt.

Primary reference: OpenBSD `urtwn_r92c_power_on` in
[if_urtwn.c](https://github.com/openbsd/src/blob/master/sys/dev/usb/if_urtwn.c)
and register definitions in
[r92creg.h](https://github.com/openbsd/src/blob/master/sys/dev/ic/r92creg.h).
The final write to register `0xfe10` is kept as an explicitly undocumented
reference step. Its internal meaning has not been established.

Validation: managed ARM release build passes. Actual Pi upload/read-back and
remote reboot passed: Wi-Fi/DHCP returned, persistent logs survived, stale
reboot requests were rejected. Installed image CRC32 `defc3948` (236,284 bytes).

## x86 timer and shared packet parsing

The PIT previously wrote `0x36` while its comment described mode 2. Named
control fields now describe the actual mode 3 square-wave configuration;
the emitted byte stays `0x36`. Reference: Intel 8254 datasheet, Control Word
Format and Mode 3 ([PDF](https://www.scs.stanford.edu/10wi-cs140/pintos/specs/8254.pdf)).
Existing x86 timer behavioral test passes.

The x86 build also exposed an existing `FrameDevice::transmit` mismatch:
the shared trait returned `bool`, while VirtIO returned `()`. The adapter now
returns `true` after its already-existing device completion. This is a build
repair discovered during verification, separate from naming changes. The
shared trait documents that local completion does not prove remote delivery.

Shared Ethernet/ARP/IP/ICMP/UDP headers now document their wire layouts.
UDP names the omitted-checksum and computed-zero encodings. IPv4 names its
version and header-word count. The checksum helper explains retaining a
high byte across buffer splits. Existing x86 and ARM QEMU UDP regression runs both passed, covering the
changed helpers and VirtIO completion path.


## Firmware, station, and DHCP follow-up

Firmware loading now names the header, banked write window, USB chunk sizes,
checksum/ready flags, and mailbox payload layout. Power-off retains the
reference operation order and labels opaque reference configurations honestly.
The station code names retry intervals and explains its connection phases.
Management-frame tags are shared with the frame parser.

Existing Wi-Fi frame tests (2), WPA2 tests (2), ARM DHCP lifecycle tests (2),
and x86 DHCP integration passed. Managed ARM release builds pass, including
power-off. The attempted upload stopped at the authentication challenge before
sending kernel data. The user subsequently confirmed the Pi is disconnected.
These changes have **not** received the required new physical validation;
the earlier power-on-only run remains the last validated hardware version.

## Pi timer and random generator

The timer names its crystal-derived count rate, scheduler tick rate, IRQ
routing, and microsecond conversions. Comments explain acknowledgement and
reading a changing 64-bit counter through two 32-bit registers. Register
reference: [BCM2836 ARM-local peripherals, section 4.11](https://datasheets.raspberrypi.com/bcm2836/bcm2836-peripherals.pdf).

The random generator names its warm-up count, available-word field and
one-second timeout. The timeout covers a whole nonce; each DATA read consumes
one word. The all-zero rejection remains only a basic failure check.
Reference: [Linux bcm2835-rng.c](https://github.com/torvalds/linux/blob/master/drivers/char/hw_random/bcm2835-rng.c), initialization and read routines.
Physical validation is pending reconnection of the Pi.

Validation: managed Wi-Fi release build and the existing Raspberry Pi QEMU
scheduler test pass. The latter checks timer preemption of two non-yielding
tasks, task completion, and the cooperative scheduling trace. It does not
exercise the physical random generator.

## Pi firmware mailbox

`mailbox.rs` now names the eight-word request fields and the separate power-on
and wait-for-stability bits. The helper name describes the ID/value exchange.
Comments explain the shared request buffer, response overwriting, channel bits,
cache publication/invalidation, and iteration-bounded polling. Unmatched replies
are still discarded, and callers still must serialize access to the shared
buffer. Layout, alignment, response checks and cache instructions are unchanged.

Reference: [Raspberry Pi firmware mailbox property interface](https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interface), buffer/tag formats, power state and clock rate.
The existing QEMU USB enumeration check passed after this change; it exercises
the USB power-on request. Physical USB/SD clock-query validation remains pending.

## Shared partition lookup

The MBR parser names the signature, entry count and sector-address width.
Its documentation separates whole-disk sector numbers from filesystem-relative
numbers. It explicitly states existing limits: primary entries only, no length
or overlap validation, and no upper bound in the offset adapter. Overflow still
returns zero-filled data through the existing errorless read interface.
Reference: [UEFI 2.10, section 5.2.1](https://uefi.org/specs/UEFI/2.10/05_GUID_Partition_Table_Format.html), legacy MBR layout tables.
The existing host `mbr_partitions` test passes: it selects the Linux entry and
reads a known payload through the translated sector address. No disk was written.

## ARM executable loading

ELF32 header offsets and segment fields now have names; the callback contract
explains file bounds, memory zero-fill responsibility, and partial effects on
failure. The ARM loader names its existing image lower bound and stack reserve,
and explains why it clears memory before copying file bytes. The x86 ELF
introduction was corrected: e_phoff locates the program-header table, which
need not immediately follow the file header. The later ELF64 audit is recorded below.
References: [ELF Header](https://gabi.xinuos.com/elf/02-eheader.html) and
[Program Loading](https://gabi.xinuos.com/elf/07-pheader.html), System V gABI.
Existing Pi QEMU userspace boot test passes: it reads an ELF from the SD image,
loads its segments, and reaches the userspace write syscall. Physical checks
remain pending; validation policy and memory protections were not expanded.

## Shared ext2 boot reader

Named superblock, inode, group-descriptor and directory-record offsets, plus
sector/block sizes and pointer counts. Comments follow name → inode number →
group/table slot → direct blocks, and explain record padding and sparse holes.
The existing twelve-block truncation and incomplete corrupt-image validation
are documented explicitly; behavior is preserved.
Reference: [Linux fs/ext2/ext2.h](https://github.com/torvalds/linux/blob/master/fs/ext2/ext2.h), on-disk structures and direct/indirect pointer constants.
The existing Pi QEMU partition test passed through MBR translation, ext2 file
loading and a userspace write syscall. This covers the combined parser changes;
it does not establish support for large files or malformed images.

## ARM virt timer delivery

Named the GIC acknowledgement ID mask, interface enable values, priority
threshold and timer interval. Comments connect the counter frequency, relative
deadline, interrupt acknowledgement and end-of-interrupt write. Register access
order and values are unchanged. The existing ARM virt QEMU timer test passes.
Reference: [ARM GIC Architecture Specification IHI 0048B](https://documentation-service.arm.com/static/5f8ff21df86e16515cdbfafe), GICC_IAR and GICC_EOIR.
This review covers the virt timer adapter; scheduler context-switch assembly
and the remaining interrupt entry path are still pending review.

## ARM task context switch

`context.rs` now documents how the task stack, saved return address and user
register bank participate in a switch. The assembly user-bank offset comes from
the actual TaskContext layout. Scheduler frame construction shares named word
indices with the switch module and explains the first return into task_entry.
The IRQ mask has its CPSR bit name; scheduling behavior is unchanged.
Reference: [Arm AAPCS32](https://github.com/ARM-software/abi-aa/blob/main/aapcs32/aapcs32.rst), core registers and stack alignment; interrupt-frame ordering is visible in `targets/armv7/src/boot.S` at `.Lirq`.
Existing Pi QEMU process-isolation regression passes, covering resumed user
processes and fault containment. Physical confirmation remains pending.

## Frame allocation and heap wrappers

The shared bitmap names its capacity and bit states; helper names say whether
frames become available or unavailable. Its API documents frame-number units,
inward rounding and caller ownership responsibilities. Both heap wrappers now
explain IRQ masking before locking; ARM names the CPSR IRQ mask and x86's stale
initialization example is corrected. The x86 boot-memory adapter was reviewed
and retained: its names already express the memory-map-to-frame conversion.
Existing ARM virt QEMU heap allocation and frame-reuse tests pass separately.
The linked-list library's internal splitting/merging was not changed.

## UART upload protocol

Named header byte ranges and CRC parameters. The event documentation makes
partial payload writes explicit and calls out that Complete carries the final
byte, which the caller must still store. Timeout timing belongs to the transport;
the decoder resets active uploads and accepts a new marker after failures.
The matching sender is `scripts/upload-pi2-kernel.py` (little-endian three-word
header and Python binascii CRC32). Existing decoder tests pass (5), including
fixed CRC vectors, timeout recovery and rejection cases; uploader tests pass (9).
No UART or Pi access occurred in this check.

## SD boot record

Named the record/version/checksum fields, copy-descriptor layout and MBR fields.
Documented confirmed, pending, attempted and running state. next_attempt and
confirm change memory; their callers in pi2_loader.rs and boot_trial.rs persist
and read back the encoded sector. This corrects the prior method comment that
implied next_attempt itself wrote to disk. Record bytes and selection order are
preserved. Existing corruption/extent, fallback/confirmation and partition-bound
unit tests pass (3); the loader-only library also compiles without kernel
features. No SD writes were made; physical validation remains pending.

## LAN9512 Ethernet over USB

The shared SMSC parser documents transmit command words, receive status and
FCS removal. The adapter names USB request fields and PHY-address packing;
comments explain internal reset, EEPROM MAC selection, receive offsets,
DATA0/DATA1 packet alternation and USB-read polling. Existing register values,
packet bytes and polling limits are unchanged.
Reference: [Linux smsc95xx.h](https://github.com/torvalds/linux/blob/master/drivers/net/usb/smsc95xx.h), TX/RX command fields and register masks.
Existing host frame tests pass (3), and the managed Pi release build passes.
QEMU's RNDIS USB network device does not exercise LAN9512 register behavior;
physical Ethernet validation remains pending reconnection.

## QEMU RNDIS adapter

Named initialization/filter fields, request IDs, USB class requests, packet
filter bits, fixed QEMU identity and frame data offsets. Comments explain the
relative offset bases and control-versus-bulk transport. Ethernet selection
names supported IDs and documents unavailable link status. Register calls,
packet bytes and validation behavior are preserved.
References: Microsoft [RNDIS packet message](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-packet-msg) and [Remote NDIS device control](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/remote-ndis-device-control).
Existing Pi QEMU UDP echo test passes through RNDIS initialization and bidirectional
bulk frames. This adapter remains specific to the configured QEMU device: fixed
MAC, no link query, and no generic device/response negotiation added.

## Pi reset/watchdog

Named the protected-write key, reset configuration bits, status register,
watchdog counts per second and short reboot delay. Documented the existing
20-bit timeout masking and the order of countdown/configuration writes.
Reference: [Linux bcm2835_wdt.c](https://github.com/torvalds/linux/blob/master/drivers/watchdog/bcm2835_wdt.c), start/stop/restart sequences. Register values and barriers are unchanged.
Managed Pi release build passes. No live reset was attempted: physical watchdog
and reboot verification remains pending while the Pi is disconnected.

## SD commands and programmed I/O

Named card command indices, controller reset/power values, sector and FIFO
word sizes, R1 ready/transfer fields and the initialization attempt limit.
Comments explain application-command prefixes and status polling with CPU IRQ
signaling disabled. Commands, access widths, copies and timeouts are preserved.
References: Linux [sd.h](https://github.com/torvalds/linux/blob/master/include/linux/mmc/sd.h), [mmc.h](https://github.com/torvalds/linux/blob/master/include/linux/mmc/mmc.h), and [sdhci.h](https://github.com/torvalds/linux/blob/master/drivers/mmc/host/sdhci.h).
Existing Pi QEMU partition-to-userspace regression passes. It validates the SD
read path; physical write/read-back and reboot remain pending.

Capacity decoding now names the Card-Specific Data fields and explains how
each format produces a count of 512-byte sectors. Clock setup names the two
parts of the ten-bit divider and shows their destination register bits.
Arithmetic, register writes and clock-enable order are preserved. Capacity
reference: Linux [mmc_decode_csd](https://github.com/torvalds/linux/blob/master/drivers/mmc/core/sd.c).
The existing Pi QEMU partition-to-userspace check passes after these changes;
it exercises the emulated card format, without proving physical clock timing
or every supported capacity format. The user has disconnected the Pi, so
physical validation remains pending.

## USB channel transfers

Internal transfer calls use `Direction` and `RetryPolicy` instead of two boolean
arguments. Named the existing attempt limit, residual-byte mask and individual
error flags. Comments connect the RAM buffer to HCDMA, the channel-enable write
to transfer submission, and the bounded polling loop to status reads. Existing
halt and cache-maintenance order is preserved.
Register field reference: Linux [DWC2 hw.h](https://github.com/torvalds/linux/blob/master/drivers/usb/dwc2/hw.h), HCCHAR, HCINTMSK and HCTSIZ definitions.

The existing Pi QEMU UDP echo regression passes through USB control and bulk
transfers. The managed Wi-Fi release kernel also builds. QEMU does not exercise
the physical adapter or the Pi cache path; physical validation remains pending
while the Pi is disconnected. Initialization, hub descriptors and DMA address/
cache explanations still need review.

Enumeration follow-up names standard/class request types, descriptor selectors,
hub-port features/status, address limits, buffer sizes and endpoint fields.
Control helpers explain SETUP, DATA and STATUS stages. The parser comment
records its existing endpoint-selection limits. Request bytes, delays and
selection behavior are unchanged. Primary references: Linux USB
[chapter 9 definitions](https://github.com/torvalds/linux/blob/master/include/uapi/linux/usb/ch9.h)
and [chapter 11 hub definitions](https://github.com/torvalds/linux/blob/master/include/uapi/linux/usb/ch11.h).
The existing Pi QEMU UDP echo regression passes and the managed Wi-Fi release
kernel builds. That QEMU setup attaches USB Ethernet directly, so physical hub
enumeration remains unverified by this run. No Pi writes or resets occurred.

Controller startup follow-up names root-port timing, host clock selectors,
FIFO regions and DMA address fields. Cache comments identify the ARM maintenance
instructions and explain whole-line ownership and the completion barrier.
The Cortex-A7's local DCIMVAC operation includes cleaning; callers must avoid
dirtying buffer cache lines during DMA. All existing operations remain in order.
References: [BCM2835 ARM Peripherals](https://www.raspberrypi.org/app/uploads/2012/02/BCM2835-ARM-Peripherals.pdf)
sections 1.2 and 15, and [Cortex-A7 TRM](https://documentation-service.arm.com/static/602cf701083323480d479d18)
table 4-8. DWC2 FIFO/HCFG field encodings use the Linux hardware header linked
above. QEMU USB UDP regression and managed Wi-Fi release build pass. Physical
cache/FIFO/clock behavior still needs validation with the Pi connected.

## Wi-Fi packet descriptors

Named the adapter's receive-header fields and the transmit fields that select
ownership, frame boundaries, queue, cipher, sequence handling and rate. The data
path's old `0x40` is the aggregation-break bit; its best-effort queue selector is
zero. Comments explain driver-information/padding skips and the descriptor XOR
checksum. USB PID toggling retains the existing DWC2 encoding.
References: OpenBSD [r92creg.h](https://github.com/openbsd/src/blob/master/sys/dev/ic/r92creg.h)
and [if_urtwn.c](https://github.com/openbsd/src/blob/master/sys/dev/usb/if_urtwn.c),
plus Realtek's Linux [RX/TX descriptor fields](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/realtek/rtlwifi/rtl8192cu/trx.h).

Managed Wi-Fi release build passes. A temporary host harness extracted the old
and new transmit-buffer construction directly from source and compared 95,664
cases: lengths 24..2016, four frame types, protected/unprotected, unicast/multicast,
and three sequence values. Descriptor and payload bytes match. The harness is
ignored under `target/runs/readability`; it performs no device operations. This
proves those encodings are unchanged, not that radio operation has been retested.
Receive arithmetic was reviewed against the reference fields; physical checks
remain pending.

## Wi-Fi key table and station rates

Named CAM command/data registers, busy/write/clear bits, key entry word layout,
security enable flags and station-rate firmware fields. The five-entry bound is
identified as Gabix station policy. Entry validity is still cleared before key
writes and set only with the final address/control word. Register operations,
values and polling limits are unchanged.
References: OpenBSD [r92creg.h](https://github.com/openbsd/src/blob/master/sys/dev/ic/r92creg.h)
CAM/SECCFG and MACID definitions, and [rtwn.c](https://github.com/openbsd/src/blob/master/sys/dev/ic/rtwn.c)
key installation and rate initialization.

Managed Wi-Fi release build passes. A temporary host harness used the actual
old/new key-installation and word-write methods with a recording register
transport. All 90 combinations of entries 0..5, key IDs 0..4 and three synthetic
key patterns produced identical results and writes. Busy reads returned clear,
so this check covers encoding/order and invalid-input rejection; it does not
exercise USB or hardware completion timing. No real key material was used.
Physical validation remains pending.

## Wi-Fi I/Q calibration

Named measured coefficient positions, validity flags, failure sentinel values,
measurement timing and calibration/correction registers. Receive correction now
uses an explicit choice; the reference tuning recipe remains visible with its
original values/order. Corrected the method description to reflect up to three
rounds, each retaining its second trigger. Programming tables were reviewed as
ordered reference data and their contract documented without changing entries.
Reference: OpenBSD [rtwn.c](https://github.com/openbsd/src/blob/master/sys/dev/ic/rtwn.c)
`rtwn_iq_calib_chain` and `rtwn_iq_calib_write_results`, with register names from
[r92creg.h](https://github.com/openbsd/src/blob/master/sys/dev/ic/r92creg.h).

Managed Wi-Fi release build passes. An ignored host harness compiled the actual
old/new calibration modules against a recording register transport. Success,
RX-failure, TX-failure and combined-failure scenarios produced identical return
values, receive-correction state and register traces. This covers the exercised
selection/application paths; analog behavior, varying measurements and transport
failures remain outside that check. Physical Pi calibration remains pending.

## Wi-Fi factory identity

Named the eFuse read supply/clocks, completion/address fields, storage bounds,
compressed block sizes, ROM signature and MAC range. Comments explain how the
record header selects a logical block and skips absent words, and why the
16-bit timeout diagnostic observes the upper register half. Source reference:
OpenBSD [rtwn.c](https://github.com/openbsd/src/blob/master/sys/dev/ic/rtwn.c)
`rtwn_efuse_read_1`, `rtwn_efuse_read` and chip identification, with register
fields from [r92creg.h](https://github.com/openbsd/src/blob/master/sys/dev/ic/r92creg.h).

Managed Wi-Fi release build passes. An ignored host harness compiled the actual
old/new identity methods with synthetic eFuse bytes. Six cases cover a valid
identity, invalid signature, multicast MAC, a truncated record stream, and each
unsupported chip flag. Decoded results/errors and setup writes match. The byte
reader was stubbed, so USB/read-completion timing still needs physical testing.

## Wi-Fi packet RAM

Named linked-list command fields, page boundaries and end marker, queue page
reservations, queue mapping, receive byte boundary and page-size encoding.
Preserved the two-OUT-pipe requirement and every write value/order. The ring
comment describes its links without assigning an unsupported receive-only role.
Reference: OpenBSD [if_urtwn.c](https://github.com/openbsd/src/blob/master/sys/dev/usb/if_urtwn.c)
`urtwn_llt_init` and DMA initialization, plus
[r92creg.h](https://github.com/openbsd/src/blob/master/sys/dev/ic/r92creg.h)
LLT_INIT, RQPN, TRXDMA_CTRL and PBP fields.

Managed Wi-Fi release build passes. An ignored host harness compiled the actual
old/new initialization methods with a recording transport. One/two/three OUT
endpoints combined with idle/stuck-busy replies gave six identical results and
traces, including the complete 256-page list setup and bounded timeout path.
Actual adapter page handling still requires physical validation.

## Indirect radio access and channel selection

Named the baseband register addresses that carry RF commands, their address/data
fields and read-edge bit, interface-dependent readback registers and delays.
Channel selection names its supported range and MAC/baseband/RF bandwidth fields;
the existing low-twelve-bit RF mask and ordering are preserved. References:
OpenBSD [rtwn.c](https://github.com/openbsd/src/blob/master/sys/dev/ic/rtwn.c)
RF read/write and channel setup, plus [r92creg.h](https://github.com/openbsd/src/blob/master/sys/dev/ic/r92creg.h)
HSSI/LSSI, BWOPMODE, RFMOD, ANAPARAM2 and RF_CHNLBW fields.

Managed Wi-Fi release build passes. A temporary recording-transport harness
compiled the actual old/new methods and compared all 256 register inputs and
channels 0..14 with three register-value patterns (including both readback modes).
All 813 result/trace comparisons pass. It does not model RF timing or radio
behavior; physical checks remain pending.

## Station transmit setup

Named factory CCK power groups and register fields, access-point address/network
type, response-rate selection, retry limits, contention fields and ACK diagnostic
filters. Interframe/ACK timing presets retain their values without assigning
unverified units. Reference: OpenBSD [r92creg.h](https://github.com/openbsd/src/blob/master/sys/dev/ic/r92creg.h)
RRSR/RL/EDCA/MSR definitions and [rtwn.c](https://github.com/openbsd/src/blob/master/sys/dev/ic/rtwn.c)
channel-group power selection.

Managed Wi-Fi release build passes. A temporary recording-transport harness
compiled the actual old/new station-setup methods and compared channels 0..14
with power values 0, 63 and 64. All 45 result/trace comparisons match. Channel
selection itself was stubbed (covered separately above); physical association
and packet transmission remain pending.

## Receiver startup

Separated startup's programming, RF interface, calibration and passive-scan
stages with source comments. Named board selection, RF interface fields,
receiver enable bits, LC calibration control/timing, MAC/filter registers,
appended receive metadata and aggregation control. Documented radio-table delay
markers. Analog/clock preset words retain their existing values and widths.
References: OpenBSD [rtwn.c](https://github.com/openbsd/src/blob/master/sys/dev/ic/rtwn.c)
initialization and [r92creg.h](https://github.com/openbsd/src/blob/master/sys/dev/ic/r92creg.h)
RF/interface and receive-control definitions.

Managed Wi-Fi release build passes. An ignored recording harness compiled the
actual old/new startup methods with the repository programming tables. Successful
startup and unsupported-board rejection produced identical results, register
traces, helper-call order and saved channel value. RF access/calibration helpers
were recorded stubs; their internal paths are checked separately above. Physical
passive reception and association remain pending.

Receiver preset follow-up names the PLL, regulator, crystal-control byte and
reset/enable fields. The reference driver's three USB-interference writes are
kept as an ordered preset sequence with their undocumented fields acknowledged.
The managed build and the same receiver trace comparison pass after this change.
References remain the OpenBSD initialization/register sources above; physical
validation remains pending.

## Shared Wi-Fi management frames

Named address ranges, information-element IDs, capability/rate fields,
authentication request/reply values and LLC/SNAP EAPOL header. Comments explain
rate units, RSN suite contents and sequence/fragment packing. The station
adapter uses the same address ranges. Reference: Linux
[ieee80211.h](https://github.com/torvalds/linux/blob/master/include/linux/ieee80211.h)
frame flags, address roles and management definitions.

Both existing `tests/test_wifi_frames.rs` checks pass (beacon truncation and
CCMP replay rejection). An ignored host comparison loads the actual old/new
modules and checks authentication, association and EAPOL builders across 6,288
buffer/SSID/capability/sequence/payload combinations. Return values and bytes
match. Managed Wi-Fi release build passes; physical association remains pending.

## CCMP frame handling

Named header/MIC sizes, packet-number width, key IDs, traffic IDs, QoS/fragment
flags and LLC/SNAP fields. Receive variables describe header bytes, packet number
and last accepted number. Comments explain per-key/per-traffic replay state and
its update after successful parsing/copy. Hardware crypto remains the caller's
precondition. References: Linux [CCMP handling](https://github.com/torvalds/linux/blob/master/net/mac80211/wpa.c)
header/packet-number conversion and the IEEE 802.11 definitions linked above.

Both existing Wi-Fi frame tests pass, including replay rejection and rejection
of aggregated QoS payloads without consuming the next packet number. An ignored
comparison of actual old/new CCMP transmit methods passes 13,130 cases across
buffer/frame lengths and consecutive calls, with equal results and bytes.
Managed Wi-Fi release build passes; physical encrypted traffic remains pending.

### WPA2 fixed handshake fields

`crates/kernel-core/src/net/wpa2.rs` now names the packet ranges, exact accepted
message words and derived key ranges used by the existing handshake. The reply
helper uses `ReplyNonce::Station` or `ReplyNonce::Zero` at each call site.
Existing synthetic handshake and RFC 3394 tests both pass; their fixture bytes
were preserved. The managed Wi-Fi release kernel builds. No new Pi run was
performed because the board is disconnected. Key-data element parsing and
cryptographic helper internals remain for subsequent review.

### WPA2 key-data decoding and unwrap

Named selectors, lengths, key ranges and masks in `net/wpa2.rs`. RFC 3394 unwrap
now names its integrity accumulator, data units and step counter while retaining
operation order and error clearing. The existing two WPA2 tests pass unchanged;
the managed Wi-Fi release build passes. Physical validation remains pending.

References: [RFC 3394 sections 2.2.2 and 2.2.3.1](https://www.rfc-editor.org/rfc/rfc3394.html#section-2.2.2)
for unwrap and its integrity value; [hostap WPA definitions](https://raw.githubusercontent.com/vanhoefm/hostap-wpa3/master/src/common/wpa_common.h)
for RSN key-data selectors and group TX/RX flag naming. The prior
`GROUP_MESSAGE_1_WITH_INSTALL` name was corrected to `WITH_TX_RX`; the accepted
value remains 0x13c2. This change does not expand the supported protocol.

### WPA2 helper contracts

The parser now states `RsnRequirement::Required` for pairwise message 3 and
`Optional` for group rekeying. Both paths still reject a mismatching RSN element
when present. Its result names the group key ID and bytes. Digest, derived-key
and MIC lengths are named; derivation comments explain the sorting and final
partial digest copy already present in the implementation. Existing synthetic
handshake/unwrap tests and the managed Wi-Fi release build pass. This completes
the shared WPA2 source review; physical validation remains pending.

### ARM VirtIO block requests

`targets/armv7/src/virt/block.rs` names the request chain and device-write flags,
feature words, modern transport version and queue selection. Comments explain
the available-index publication and used-index poll. The poll has no timeout;
this audit preserves that behavior. The existing QEMU block-read and ext2 file
read tests pass (`test_arm_virt_reads_block.sh`, `test_arm_virt_reads_file.sh`).

Reference: [VirtIO 1.2](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html),
sections 2.7, 4.2 and 5.2. This QEMU driver does not operate the physical Pi SD
controller; these checks do not replace the pending physical Pi checks.

### ARM VirtIO network buffers

`targets/armv7/src/virt/network.rs` now names both TX and RX storage consistently,
identifies descriptor roles, and explains the transfer of buffer ownership.
Transmit remains synchronous with an unbounded poll; receive checks once and
returns `None` when empty. The existing QEMU UDP echo test passes, including its
DHCP exchange and reply payload/checksum verification. This is QEMU-specific
evidence, independent of the pending Pi Wi-Fi hardware checks.

Reference: [VirtIO 1.2 section 5.1.6](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html)
for the modern packet header, zeroed transmit fields and single receive buffer
when mergeable buffers are not negotiated. Numeric values, descriptor access
permissions, copy lengths and barrier positions are preserved.

### x86 PCI discovery

`src/drivers/pci.rs` names scan limits, configuration offsets and BAR fields.
Comments now describe implemented discovery, endpoint-only six-BAR decoding,
and the obsolete below-1-MiB memory encoding. The existing diagnostic string
for unsupported memory types remains unchanged. Address packing already has
a bit-by-bit description; the class-code lookup remains a readable value table.

The existing x86 known-block QEMU test passes. It exercises the configured QEMU
device topology, not every PCI BAR encoding or bridge topology. Field meanings
were checked against [Linux PCI register definitions](https://raw.githubusercontent.com/torvalds/linux/master/include/uapi/linux/pci_regs.h).

### x86 VirtIO capability transport

Named region-relative offsets and MSI-X table size, BAR index, address offset,
entry size and control fields in `src/drivers/virtio/transport.rs`. The capability
result documents the unused notification multiplier read on other capability
types. The parser still trusts capability lengths and list termination.
Existing x86 QEMU disk-read and network-frame tests pass; malformed-device cases
are outside those checks. Queue setup and device initialization remain open.

References: [Linux PCI MSI-X fields](https://raw.githubusercontent.com/torvalds/linux/master/include/uapi/linux/pci_regs.h)
and [VirtIO 1.2 section 4.1.4](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html).

### x86 VirtIO queue mechanics

Named ring offsets/entry widths and block buffer lengths, reused the shared
frame size, and renamed the device-write permission. Removed disabled print
blocks. Corrected the 64-entry available-ring size (134 bytes), alignment and
completion-length comments. Volatile access and atomic publication are explained
separately; the driver still uses the same acquire/release operations.

Existing x86 disk-read and network-frame QEMU tests pass. These cover the current
device configuration and ordinary submissions, without establishing malformed
queue safety or concurrent-producer support. Submission requires serialization.
Reference: [VirtIO 1.2 sections 2.7 and 5.2.6](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html).

### x86 VirtIO block driver

`src/drivers/virtio/block.rs` names queue selection, MSI-X routing offsets,
feature words and buffer sizes. Comments explain the interrupt-assisted read
loop, completion status, notification address and retained allocations. Each
read still retains three frames; this readability pass does not reclaim them
or change request behavior. The existing known-block QEMU test passes.

The transport/queue references above apply to these fields. Review covered the
whole block source; x86 networking initialization remains pending. This test
exercises the attached QEMU disk, without proving error-device handling or
physical Pi operation.

### x86 VirtIO network driver

Named queue IDs separately from MSI-X entry IDs, feature words, control values
and table offsets in `src/drivers/virtio/network.rs`. Comments explain zeroed
transmit metadata, the interrupt-assisted completion loop and receive copying
before buffer reuse. The shared frame size replaces repeated allocation units.
The 64-byte transmit padding policy and two retained frames per transmission are
explicit existing limitations; behavior is preserved.

The existing x86 QEMU network-frame test passes. Combined with the prior disk
and network checks, this completes source review of the PCI/VirtIO driver group.
Other source groups and physical Pi validation remain pending. The previously
recorded VirtIO 1.2 and Linux MSI-X references cover the named fields.

### x86 interrupt controllers

Named APIC enable fields, mapping mask/size, spurious-vector setting and EOI
value. MSI address documentation identifies physical destination APIC ID 0.
Initialization preserves mode bits and requires xAPIC already selected. PIC
comments now match its timer-only unmasking, expressed with named masks.
The existing x86 QEMU timer test passes; it also boots through disk loading.
Spurious-interrupt delivery and alternative APIC modes are not tested by it.

References: [Linux APIC definitions](https://raw.githubusercontent.com/torvalds/linux/master/arch/x86/include/asm/apicdef.h)
and [x86 MSI message layout](https://raw.githubusercontent.com/torvalds/linux/master/arch/x86/include/asm/msi.h).

### x86 saved task stacks

`context::saved_stack` names register-word positions, the return address, initial
alignment gap and extra fork state. `Task::new`, `spawn_fork_child` and the fork
assembly consume those names. The existing six-pop/return instruction sequence
and all numeric offsets are preserved. Comments distinguish the Rust trampoline
entry from the fork assembly entry and correct the initial-stack diagram.

`test_fork_resumes_both_processes.sh` passes after granting its QEMU process
access to the configured local forwarding port; the first launch failed before
boot. Evidence covers startup and parent/child resumption, not the full remaining
scheduler and exception-entry review.

### x86 boot and user entry

Named boot settings and CPU flags; initial user entry takes the selectors
created by GDT setup. The installed `x86_64` crate's `GlobalDescriptorTable::append`
constructs selectors with the descriptor's privilege level, retaining the
current user CS/SS values. GDT comments reflect scheduler updates of TSS.RSP0
and the separate software SYSCALL stack switch.

Corrected invalid inline-assembly annotations encountered during review: the
SSE-enable OR clobbers flags, and the IRETQ sequence pushes memory and never
returns. This is a compiler-contract correction alongside the readability work.
Existing user-mode boot and timer QEMU tests pass. These cover startup and
ordinary interrupts; the remaining interrupt-wrapper review is separate.
Flag references: [Linux x86 processor flags](https://raw.githubusercontent.com/torvalds/linux/master/arch/x86/include/uapi/asm/processor-flags.h).

### x86 syscall entry

The saved-argument offsets now name their syscall argument and original register.
The seventh C argument offset accounts explicitly for its reserved stack slot.
STAR is computed from the same GDT selectors used for user entry, preserving
its current value. Stack comments now describe the existing 16-frame/64-KiB
allocation. Ordinary syscall return and the smaller fork snapshot have separate
contracts; no fork snapshot expansion or register-order change was introduced.

Existing fork-resumption and UDP echo QEMU tests pass. These exercise user entry,
fork return, and socket argument forwarding; they do not prove every register
value under arbitrary nested scheduling. The scheduler/global snapshot policy
remains part of the broader process review.

### x86 page-table dimensions and copying

`src/arch/x86_64/paging.rs` names the 512-entry table size and upper-half
boundary. Frame allocation, clearing, copying and stepping use the existing
shared 4096-byte frame constant. The clone walk retains the same iteration
bounds and branching. Comments explain that high-half subtrees and low-half
supervisor frames remain shared, while low-half 4 KiB user frames are copied.
Huge-page entries remain shared regardless of their user flag.

Corrected obsolete claims that `new()` copies user mappings and that sharing
kernel frames depends on their being read-only. Address-space handles have no
automatic reclamation. Caller serialization remains required: this slice does
not establish exclusive ownership or add locking to per-address-space operations.
Existing fork-resumption and exec-replacement QEMU tests pass; raw output stays
under ignored `target/runs/readability/`. Synchronization and aliasing review
remains open, as does physical validation of earlier Pi changes.

### x86 interrupt and exception wrappers

`src/arch/x86_64/interrupts.rs` uses named stack-word sizes, padding and
frame offsets. Timer-vector selection uses the PIC definition; fault-bit
printing uses the installed x86_64 crate's `PageFaultErrorCode` names.
Instruction order, padding amounts, vectors and printed bit values are preserved.
Comments trace timer register saves through the scheduler and distinguish the
network interrupt's wakeup from receive processing in the network task.

Stack layout was checked against Intel SDM Vol. 3A section 6.14.2,
[64-Bit Mode Stack Frame](https://cdrdv2-public.intel.com/835754/253668-sdm-vol-3a.pdf).
The review exposed existing correctness limitations, now stated in the source:

- Breakpoint entry calls Rust without preserving caller-saved registers.
- Page-fault entry subtracts eight bytes from an already aligned 48-byte CPU
  frame, misaligning its C call. This change names the existing padding and
  records the defect; it does not repair the entry sequence.
- The fatal page-fault diagnostic unconditionally reads the original demo's
  user address, which may be unmapped and cause another fault.
- Interrupt wrappers preserve no SIMD/floating-point state.

Existing timer and UDP echo QEMU checks cover ordinary boot, timer progression
and network delivery. They do not exercise breakpoint or page-fault recovery,
prove SIMD preservation, or resolve these recorded defects. Exception repairs
need focused behavioral tests in a separate correctness change.

### x86 ELF64 parsing and executable loading

`src/elf.rs` names the byte offsets of identification, ELF-header and
program-header fields. These are offsets into serialized bytes, independent
of Rust struct layout. `src/exec.rs` uses the shared frame size for page
arithmetic and identifies the file-supplied program-header stride.

The comments describe existing limits: the format predicate checks class,
byte order, type and machine; versions/OS ABI/header sizes are not validated.
The loader trusts table/segment ranges and arithmetic. It enforces PF_W only,
maps the stack executable, and leaves old user pages outside replacement
ranges in the cloned address space. Replaced memory is not reclaimed here.
These are recorded limitations, not new behavior or claims of robust input
validation. Header field offsets, sizes, iteration and copy ranges are unchanged.

References: gABI [ELF Header](https://gabi.xinuos.com/elf/02-eheader.html)
and [Program Loading](https://gabi.xinuos.com/elf/07-pheader.html).
The existing exec-replacement QEMU test passes through initial ELF boot and
loading the replacement program. It does not establish malformed-file handling
or full address-space replacement; those require separate correctness work.

### Syscall user pointers, files, time and process entry

Renamed `is_user_range` to `is_low_half_range` at every syscall call site.
Its contract now states its actual checks: addition overflow and the numeric
low-half bound. It does not establish page presence, user permissions,
writability, alignment or Rust slice validity. Path decoding constructs a
256-byte slice before searching for NUL; the full range must be readable.
The current handlers still rely on those unchecked memory requirements.

File handlers name stdout/stderr and explain cached inode writes and shared
file cursors. Time conversion derives nanoseconds per tick from named units
while preserving 100 Hz and 10,000,000 ns/tick. Process handlers name the
wait-any sentinel and initial user flags, explain the raw i64 wait result and
unchecked result pointer, and describe the SYSRETQ arguments for successful
exec. The fork snapshot comes from global entry storage: disabling interrupts
inside fork does not establish that the snapshot survived earlier preemption.
That scheduler/entry interaction remains open correctness work.

Existing wait-status and monotonic-time QEMU tests pass. No pointer validation,
fork snapshot synchronization or syscall ABI behavior was changed. These tests
exercise supported calls with valid memory; they do not validate bad-pointer
handling or resolve the documented concurrency limitations.

### Socket syscall ABI and waiting

`src/syscall/socket.rs` names sockaddr_in field ranges, total size and socket
length width, retaining native-endian family and network-endian port/IP bytes.
Protocol selection reuses shared IPv4 protocol constants. Named values cover
poll wait modes, listen backlog cap, configuration operations, fixed active-open
port and the existing deterministic sequence-number calculation. The poll
closure result is named `ready_count`, matching its Option<count> value.

Comments identify existing constraints: raw user storage must be accessible;
short sender-address buffers are rejected; a receive can consume the datagram
before sender-address output fails; active-open TCP uses one global descriptor
number without a process identity. These behaviors and all numerical values
are unchanged. The sequence calculation is predictable, not a source of
cryptographic randomness.

Reference: Linux UAPI [in.h](https://raw.githubusercontent.com/torvalds/linux/master/include/uapi/linux/in.h)
for sockaddr_in layout and protocol numbers. Existing QEMU poll and passive TCP
echo checks pass; the latter covers bind/listen/accept and payload exchange,
not the separate global active-open connection path. No new tests were added.

### Scheduler task states and lifecycle helpers

`src/task/scheduler.rs` gives the repeated Ready/Running decision one
`TaskState::is_runnable` predicate. For the existing five enum variants it
selects exactly the states previously accepted by the three exclusion checks.
Stack allocation uses the shared frame size, retaining all strides and sizes.
Comments explain that block helpers change state before the caller yields,
wake changes Blocked to Ready, and descriptor callbacks hold PROCESS_TABLE
rather than SCHEDULER. Callback reentry and scheduling restrictions are explicit.

`src/task/lifecycle.rs` documents that exit/replacement leave interrupts disabled
for their caller's next step. Reaping removes Process state while retaining Dead
Task storage and stack allocations. The idle branch of yield_now currently
enables interrupts and returns without rechecking the current task's state;
this review documents that behavior rather than changing its semantics.

Existing wait-status and poll-ready QEMU tests pass, exercising child exit/reap
and socket wakeups. These do not establish correctness of every preemption gap,
idle-return state or memory-lifetime invariant. Remaining task-creation and
process ownership review is still open.

### Process table and task creation

Reviewed `src/process.rs` and the remaining creation/fork sections of
`src/task/scheduler.rs`. Comments describe monotonically allocated PID slots,
permanently retained holes, placeholder address-space handles, inherited
file/socket objects, and the actual low-half clone versus shared high-half
mapping rules. Fork stack placement now explains the pid+1 entry-stack offset
and its unused slots. Kernel-task startup explains its interrupt transition and
panic on return without historical implementation claims.

Existing limitations are explicit: ProcessName truncates by bytes and can
split UTF-8 before an unchecked string conversion; kernel-task spawn sets a
parent without updating that parent's child list; kernel-task stack creation
uses the boot-time mapper and requires its range visible in the active table.
These comments do not fix those behaviors. Frame ownership/reclamation and
page-table synchronization remain separate open work.

The existing fork-shared-file-cursor QEMU test passes, observing inherited
file access and cursor sharing. This slice changes comments only; it adds no
runtime checks and does not establish safety for arbitrary process names or
all kernel-task spawn contexts.

### VFS cache and descriptor ownership

`src/fs/mod.rs` names the one-byte name length, eight-byte inode and combined
fixed width of its internal directory records. The directory parser names
entry/name/inode boundaries; the byte format and truncation behavior remain
unchanged. Path resolution uses the existing ROOT_INO constant.

Corrected historical comments about missing path lookup, future fork inheritance
and access modes that File does not store. The source now describes shared
open objects, separate descriptor slots, cache retention after close, read-only
filesystem trait methods, and path traversal before the final cache lookup.
Existing namespace limitations are explicit: in-memory and disk inode numbers
share a registry, and replacing a mount leaves cached objects from the old one.

The existing QEMU file-read test passes through path resolution, open and read.
It does not test mount replacement or the standalone cached-directory decoder;
the latter's renamed width expressions retain 1 + name length + 8 and the same
field slices. No disk-write or cache-invalidation behavior changed.

### x86 ext2 adapter storage units

`src/fs/ext2.rs` names sector/superblock/read-block sizes, base inode and group
descriptor widths, and direct/indirect pointer positions. The same two sectors
and slices are read as before. Field-offset parsing remains a separate review.
Corrected the directory file-type description: its enumeration differs from
inode mode bits. References were checked against Linux
[fs/ext2/ext2.h](https://raw.githubusercontent.com/torvalds/linux/master/fs/ext2/ext2.h).

Comments identify the adapter's existing limits: fixed 1 KiB data reads,
group-0 inode-table selection, direct blocks only, truncation at the first zero
file pointer, and no feature/block-size rejection at mount. On-disk self/parent
directory entries are omitted from listings but remain available through lookup.
The existing QEMU file-read test passes for the generated supported image.
It does not validate other block sizes, inode groups or sparse/indirect files.

### x86 ext2 serialized field parsing

The remaining parser literals in `src/fs/ext2.rs` now name superblock,
group-descriptor, inode and directory-entry fields. The block-pointer stride
uses the serialized u32 width; permission display names its existing mask.
Offsets and field widths retain their original values, checked against the
Linux ext2 structure definitions cited above.

The parser contracts now identify short-header panics, empty names on truncated
directory payloads, unchecked record boundaries/geometry, and low-word-only
file sizes. No validation policy changed. The existing QEMU file-read test
passes through these parsers on the supported image; malformed-image handling
is not established by that test. The broader audit remains incomplete.

### Network service and ARP waiting

`src/net/device.rs` names the ARP iteration limit, unregistered task sentinel,
broadcast addresses and diagnostic row width. Values and control flow remain
unchanged. Comments trace task registration/enabling, device-lock ownership,
receive dispatch, and the separate ARP request/wait phases.

The review corrects overstatements: try_lock bounds acquisition only; callbacks
can wait. The receive path may transmit replies through the VirtIO driver,
which uses enable_and_hlt, so an outer without_interrupts closure does not prove
the entire drain atomic. A rejected Ethernet frame can make poll_rx return None
with other queued frames remaining. UDP acceptance permits omitted checksums.
The service wrapper discards the driver's transmit success flag; Some(()) on
IPv4 send is not delivery confirmation. ARP's 500 iterations count wake/check
cycles, not elapsed milliseconds, and miss paths enable interrupts on return.

The existing QEMU UDP echo test passes across receive dispatch, socket delivery,
ARP and reply transmission. It does not prove concurrency safety under every
wake/transmit ordering or precise timeout duration. These documented limitations
were preserved; no new tests or runtime policies were introduced.

### UDP queue ownership and runtime network state

Reviewed `src/net/socket.rs`, `src/net/arp.rs` and `src/net/mod.rs`. Comments
now state that ports are global weak bindings; strong descriptor handles own
sockets; dead registry entries remain; receive payloads enter an unbounded FIFO;
one receive waiter can be overwritten while poll waiters are deduplicated.
Registration does not block a task itself, and delivery releases socket locks
before scheduler wakeups. Delivery success means queued, not read by userspace.

ARP cache comments describe sender learning, previous-value returns, lack of
expiry/interface keys and the distinction from authentication. Configuration
comments identify lease duration as stored metadata rather than a renewal timer,
and document the current destination filter. These are source-derived contracts;
no behavior changed. Cargo check passes. Existing behavioral tests were not
repeated for this comment-only slice, and no queue bound, aging or concurrency
repair is claimed.

### TCP wire fields

`src/net/tcp.rs` names fixed header ranges, Data Offset word units and shift,
receive-window policy, SYN MSS option fields, and IPv4 checksum pseudo-header
fields. The advertised 4096-byte window and 1460-byte MSS are unchanged; the
latter still encodes as 05 b4. The parser checks checksum and overall header
length, then skips option bytes without negotiating their contents.

Wire layout reference: [RFC 9293 sections 3.1–3.2](https://www.rfc-editor.org/rfc/rfc9293.html#section-3.1).
Existing QEMU TCP echo passes through SYN/ACK exchange and payload transfer.
This covers the changed encoder/parser in the passive connection path; state
transitions and retransmission logic remain a separate review. No protocol
behavior or new tests were added.

### TCP send accounting and retransmission timing

`src/net/tcp.rs` names the integer RTT-estimator weights, variation multiplier,
initial variance divisor, backoff multiplier and initial peer-window assumption.
Segment buffers use the existing SYN header size. All arithmetic, rounding,
bounds and packet sizes retain their values.

Comments explain byte-based sequence accounting, cumulative ACK removal,
partial-entry retention, the fixed send-window cap, and timestamping when
outbound work is prepared. Retransmit polling updates queue state before the
caller sends; it has no retry-count limit. Only the global active-open path
uses this queue. Passive connection state review remains open.

Formula reference: [RFC 6298 sections 2, 3 and 5](https://www.rfc-editor.org/rfc/rfc6298.html).
The existing QEMU TCP-loss test passes with its external peer, verifying retry
and response after loss. This confirms the tested path, not full RFC compliance
or retransmission support for passive sockets. No new tests were introduced.

### TCP passive connection ownership and state transitions

`src/net/tcp.rs` names the passive initial-sequence generator constants and
reuses SEND_MSS for segmentation. Comments trace listener weak references,
connection-table strong ownership, completed-handshake queue admission,
receive-buffer consumption and preparation of outbound segments. Existing
values and control flow are unchanged.

Source review records the current limitations: connections remain in the table
after closing; TIME_WAIT has no expiry; backlog bounds completed accepts only;
a full queue can strand an established connection; the handshake ACK branch
skips accompanying payload. Passive sends have no retransmission queue or peer
window accounting. Passive teardown advances on an ACK flag without checking
that it acknowledges the FIN. Receive queues are unbounded, out-of-order data
gets no ACK, and RST handling is absent. The active receive path also lacks FIN
handling. These are existing implementation gaps, outside this readability edit.

The existing QEMU TCP echo test passes, including its passive connection flow.
No new tests were added. This result covers the tested path; it does not verify
the unsupported cases above. Physical Pi verification remains pending while
the board is disconnected.

### ARM mapping control and dimensions

`targets/armv7/src/memory.rs` names MMU/cache enable bits, coherent-request
control, table alignment, section address mask, execute-never bit, process
section count and stack-section index. Values and instruction order are
unchanged. Register comments explain TTBCR, TTBR0 and DACR setup.

Control-bit reference: [Arm Cortex-A7 TRM, system control registers](https://documentation-service.arm.com/static/5e8f089d88295d1e18d3b906),
sections 4.3.27–4.3.28. The table layout and existing cache-attribute encoding
still need the deeper memory ownership/cache-assumption review.

Helper comments describe the source's actual boundaries: allow_user_section
requires a valid Pi alias supplied by its caller; allow_user_range silently
returns on overflow and touches a section for an unaligned zero-length range.
process_range_is_valid checks only interval arithmetic, including a zero-length
range at the end. The existing QEMU ARM memory-mapping and process-isolation
tests pass. Physical Pi validation remains pending while disconnected.

### ARM process isolation probe

`targets/armv7/src/process.rs` and `process_probe.S` name the shared data-field
offsets, access kinds, probe stack offsets, minimum timer switches and syscall
numbers. Outcome arrays use PROCESS_COUNT. The setup contract explains that
it switches tables and clears the stopped process's entire allocation.
The fault comparison names the saved mode, split status field, section
permission fault and write flag. Numeric values and conditions are preserved.

References: [Arm DFSR fields](https://arm-software.github.io/CMSIS_5/Core_A/html/group__CMSIS__DFSR.html)
and [Cortex-A7 TRM fault-status encodings](https://documentation-service.arm.com/static/5e8f089d88295d1e18d3b906).
The existing QEMU isolation test passes its baseline, other-process read/write,
kernel read and stack-execution cases. The Pi hardware build passes. This is
source/build evidence for the Pi; the disconnected board has not run this edit.
The fixed probe ABI still has paired Rust/assembly definitions, linked by source
comments and exercised by the existing test. General process lifecycle review
remains open.

### ARM user entry and bootstrap ELF loading

`targets/armv7/src/user.rs` names the three processor status values and flat
header entry-word offset. Assembly comments trace the r0/r1 arguments, user
stack setup through System mode, SPSR_svc setup in Supervisor mode and exception
return into user code. Instruction order and numeric values are unchanged.
References: [Arm CPSR mode values](https://arm-software.github.io/CMSIS_6/v6.2.0/Core_A/group__CMSIS__CPSR__M.html)
and [Arm exception return](https://support.arm.com/documentation/dui0471/m/handling-processor-exceptions/return-from-an-exception-handler?lang=en).

Source contracts state that the flat header checks only its magic. ELF loading
grants section-wide user read/write permission, ignores segment permission
flags, returns the parsed entry without checking its destination, and can leave
partial copies/mappings on failure. Instruction-cache synchronization is a
caller requirement; load_elf does not perform it. User entry initializes the
stack and processor status, while other registers keep their current values.

The existing QEMU userspace-write test passes through the flat-header and user
entry path. ELF loading received comments only; this test does not establish
malformed-ELF safety. Physical Pi checks remain pending.

### ARM syscall adapter

`targets/armv7/src/syscall.rs` names the syscall/result register indices, saved
register count, getpid number, error value, stdout descriptor, output-size limit
and semihosting exit fields. Write bounds reuse the identical process-address
constants. The UserFrame comment matches boot.S .Lsvc's r0-r12/LR save order;
SPSR remains separate. All values and dispatch decisions are preserved.

Comments explain scheduled-process exit/getpid and reboot rejection, standalone
exit through the Pi trampoline or QEMU semihosting, and synchronous UART output.
The standalone exit ignores the requested status. Write checks address arithmetic
but does not inspect mappings or recover from a read fault. These are existing
limits. This review does not certify the exception assembly's full context-save
or interrupt behavior, which remains separately open.

Reference: [Arm semihosting specification, trap encoding and SYS_EXIT_EXTENDED](https://github.com/ARM-software/abi-aa/blob/main/semihosting/semihosting.rst).
Existing QEMU userspace-write and process-isolation tests pass. The Pi hardware
build also passes; physical execution is pending while the board is disconnected.

### ARM scheduler lifecycle

`targets/armv7/src/scheduler.rs` names the dispatcher sentinel and preemption
probe round count. Comments follow the source's switch order: choose mappings,
resume a task stack, regain the dispatcher stack with IRQs masked, then select
boot mappings. The per-call saved IRQ setting stays on each suspended stack.
The state machine has Empty, Ready, Running and Finished only; there is no
blocked/sleeping state. With IRQs disabled, a non-yielding task prevents return.

The static stack arrays live for the kernel's entire run. reset_finished_tasks
checks that all slots are inactive before permitting their reuse; it does not
free stack memory. Process exit abandons the call chain without running local
destructors. spawn_process requires a prepared, unique process identity from its
caller and does not validate that requirement. These comments describe existing
behavior; names preserve the previous numeric values.

Existing QEMU cooperative/task-preemption and process-isolation checks pass.
An initial local sentinel-replacement typo failed compilation and was corrected
before both checks were rerun successfully. Exception-wrapper review and deeper
memory/cache assumptions remain open, as does physical Pi verification.

### ARM exception assembly

`targets/armv7/src/boot.S` names processor modes, core-index mask, exception
return-address adjustments, call alignment and the abort frame's saved-status
offset. The offset follows SRS's PC/status pair plus the 14-word integer push.
Comments tie the SVC frame to syscall::UserFrame and trace SPSR preservation
across IRQ preemption and the RFE return. Numeric values and instruction order
are unchanged.

The abort handlers terminate a process or halt, so their rounded stack pointer
is never restored. SVC entry assumes an aligned kernel stack and no nested SVC.
These wrappers save integer state only; floating-point/NEON context preservation
is absent. Startup GPIO/UART setup remains a separate review.

Reference: [Arm Cortex-A programmer's guide, exception handling](https://documentation-service.arm.com/static/6893ad23e7f7ce6150e88d37),
chapter 12, for saved status and exception return adjustments. Existing QEMU
isolation and task/preemption tests pass through these wrappers, and the Pi
hardware build passes. Physical Pi execution remains pending.

### ARM boot serial setup and console

`targets/armv7/src/boot.S` names the GPIO14/15 function fields, PL011 register
offsets, FIFO/enable fields, existing baud divisors and semihosting exit values.
GPIO alternate-function 0 routes TXD0/RXD0. Divisors retain 1 and 40; the code
does not query UARTCLK, so comments avoid asserting a universal baud rate.
The initialization order and emitted values remain unchanged.

`console.rs` names word-indexed flags and FIFO bits. Its contracts state that
initialization is external, handles do not serialize access, receives discard
error bits, and transmit polls without timeout until FIFO room exists. Returning
from write_byte means enqueued, with no wait for physical transmission to finish.
The safe constructor accepts arbitrary bases; validity remains a caller
requirement, not a checked guarantee.

References: [Arm PL011 register definitions](https://github.com/ARM-software/arm-trusted-firmware/blob/master/include/drivers/arm/pl011.h)
and [BCM2835 peripherals, GPIO and UART chapters](https://pip-assets.raspberrypi.com/categories/579-raspberry-pi-zero/documents/RP-008249-DS-1-bcm2835-peripherals.pdf).
The existing QEMU userspace-write test passes; Pi hardware and UART-user builds
pass. Physical pin routing, baud rate and receive behavior remain pending Pi
verification while the board is disconnected.

### Loader-to-kernel trial handoff

`boot_record::handoff_field` defines the existing 28-byte little-endian RAM
layout once for `pi2_loader.rs` and `boot_trial.rs`: magic, boot-record sector,
copy index, image length/CRC and message CRC. Ranges and checksum coverage retain
the original values. This RAM message describes the attempted image; the SD
boot record remains the persistent decision state.

Trial comments trace consuming/clearing the handoff, checking the running image
against its CRC, watchdog refresh and confirmation write/read-back. A failed
read-back can follow a successful physical write, so failure keeps local trial
state/watchdog active without claiming the disk is unchanged. With no active
trial, confirm is a no-op success. Calls rely on single-core boot/service access.

Both SD loader and managed kernel builds pass. All three existing host boot-record
tests pass (record validation, fallback ordering, partition bounds). Those tests
cover persistent record behavior, not physical RAM handoff or SD persistence.
No new tests or device writes were added; physical validation is pending.

### SD loader selection and recovery

`targets/armv7/src/pi2_loader.rs` names the UART override window, boot watchdog,
sector size and watchdog refresh byte interval. The source now explains that
attempt state is persisted before loading, image read/CRC failure selects another
copy, and record I/O failure returns to UART immediately. Streaming copies bytes
into RAM before the final CRC decision; execution waits for a matching checksum.
The running kernel performs confirmation later. Values and control flow remain
unchanged.

Raw destination writes rely on their caller's bounds. A returning kernel is
treated as failed, but that continuation requires the loader's stack/mapping
environment to remain usable. UART waits are polling loops; write/flush have no
local timeout, reads discard error flags, and the timed read gives arriving data
priority when checking its deadline.

The existing firmware-address test passes for the UART loader; the SD loader
build passes separately. No physical SD boot or recovery was exercised in this
slice. Loader startup assembly and remaining installer/management paths remain
open, along with disconnected-Pi validation.
