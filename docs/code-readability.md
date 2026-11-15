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
  - [ ] Remaining shared frame/security fields and station-frame offsets
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
  - [ ] Boot/exception assembly, process operations, memory and syscalls
- [ ] ARM loader, SD update/install, authenticated management, and diagnostics
- [ ] ARM VirtIO adapters and hardware probes
  - [x] QEMU virt timer/GIC: named interrupt fields, counter interval and acknowledgement sequence
- [ ] Shared network parsers and state machines, including WPA2
  - [x] Ethernet, ARP, IPv4, ICMP, UDP, and checksum helpers: wire layouts, abbreviations, meaningful field values, and split-buffer checksum explanation
  - [x] DHCP options/header fields and ARM lease retry policy; stack dispatch contracts
  - [ ] Remaining Wi-Fi/WPA2 fields
- [ ] Shared ELF, partitions, ext2, memory, loader protocol, and boot records
  - [x] Boot record: field layout, copy-selection state, persistence ownership and MBR bounds
  - [x] UART upload decoder: header fields, incremental storage events, checksum and timeout ownership
  - [x] Shared frame bitmap and both heap wrappers: frame/address units, availability bits, IRQ locking
  - [x] Shared ext2 boot reader: disk fields, inode/group arithmetic, directory records and direct-block limit
  - [x] ARM ELF32 parser: named header offsets, segment/callback contracts; x86 ELF introduction corrected (remaining x86 parser review pending)
  - [x] MBR primary-entry lookup and offset adapter: named layout fields and explicit validation limits
- [ ] x86 boot, paging, context switching, interrupts, PIC/PIT/APIC, and syscalls
  - [x] PIT: named control-word fields and corrected the mode description
- [ ] x86 PCI and VirtIO drivers
- [ ] x86 scheduling, process lifecycle, file operations, and network stack
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
need not immediately follow the file header. Its detailed parser audit remains.
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
