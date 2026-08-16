# Raspberry Pi 2 UART payload baselines

Each file in this directory is immutable after its physical result is recorded.
New stages receive a new filename. The SD-resident diagnostic loader writes each
payload at `0x00100000` after validating its CRC32.

| Stage | Artifact | Bytes | SHA-256 | Physical result |
|---|---|---:|---|---|
| 0 | `stage0-handoff-return.img` | 32 | `b302ec7bda3381fadcf702ca02696c35bce3b57d9d86a0e3f3b5f7eb5e4724fa` | `P`, then `HANDOFF RETURNED` |
| 1 | `stage1-hyp-to-svc.img` | 96 | `14c1d4507ae1020fa566d9a9506942bd1077c58e8d556445ae1641fab090d445` | Initial CPSR mode `0x1A`; payload emitted `S` after reaching SVC mode `0x13` |
| 2 | `stage2-bss-clear.img` | 232 | `8346b242fcfa476e65764761daabfa98f0320849920e1a7da6c4f261b1492185` | Poisoned and cleared 64 B of NOBITS memory, emitted `B`, then watchdog-reset to `GABIX LOADER READY` |
| 3-failed | `stage3-rust-allocator.img` | 1,600 | `07958c3b4dcc503c6a003794332181034da278f5e46c8eb781477e165c2eee35` | `HANDOFF BRANCH`, then silence; combined Rust, BSS, allocator, allocation, and success-only reset, so it was intentionally split |
| 3a | `stage3a-rust-entry.img` | 164 | `8d0c1ebf4a0ec032b35f3a40c87539610ad08ec6fb0854574ff0cec94a111f25` | Minimal Rust entry emitted `R`; loader-ready followed while a manual cycle also occurred |
| 3b | `stage3b-rust-bss.img` | 396 | `9c7273d7f006763f374049397dd71a99c6eabf0a5dc1e06406c3f9f5df5fdaf2` | Whole BSS was poisoned, cleared, verified from Rust as zero, emitted `Z`, then autonomously reset |
| 3c-short-watchdog-failed | `stage3c-allocator-init-short-watchdog-failed.img` | 888 | `0b7eb92184f8fa8cc0d4e06e20e5aab9c1411d4ce82fcb3d5dbdcd165e05eb36` | No `I`; watchdog value `80` expired after about 1.2 ms |
| 3b-large-short-watchdog-failed | `stage3b-large-bss-short-watchdog-failed.img` | 292 | `26eee08f12321101218ec38ff19ed22003660a8724a3e71d7d890e7c28a5ba2a` | Reset before `H`; watchdog value `80` was only about 1.2 ms |
| 3b-large | `stage3b-large-bss.img` | 292 | `697fc4f8f13e4b62fe8aa4b4fb6b53ce7db97d0edcb27d37b384fad3ba09cc6a` | Cleared and sampled 1 MiB BSS, emitted `H`, then autonomously reset |
| 3c | `stage3c-allocator-init.img` | 888 | `e6a57abf0f1c87ccb45472f76cd2d476b4201784051cf9c6f9f541c4dea8ab7f` | No `I`; five-second deadman reset confirmed allocator initialization still hung |
| 3c-atomic-pre-MMU | `stage3c-atomic-before-mmu.img` | 268 | `f2251e71eaf285902cbd8d5faf3cc977f6f8cece23e7b7a38207f352a7b645fd` | No `L`; emitted zero-byte stream during atomic path, then deadman reset |
| 4a | `stage4a-current-mmu-atomic.img` | 404 | `a47f435670158867fcbb58ed45b2096adccf917768e420fd7fc9012adfe55420` | Emitted `M` but no `L`; strongly ordered identity map enabled, then atomic path failed and reset |
| 4b | `stage4b-normal-ram-mmu-atomic.img` | 424 | `67aa113786488c5ca219448ecebaa7de9008aedc6c3d2d80c7b1af88bcfa0e80` | Emitted `M`, then one non-ASCII byte and no `L`; Normal shared non-cacheable RAM alone did not make the exclusive access complete |
| 4c | `stage4c-smp-normal-mmu-atomic.img` | 484 | `1b4f03ce6adcabc641d28f066eef313eaad8a6a891e0c4d92a6e5d4696117350` | Emitted `S` and `M`, then no `L`; ACTLR.SMP read back as enabled and the MMU probe passed, but the atomic loop did not complete before the deadman reset |
| 4d | `stage4d-smp-normal-mmu-ldrex.img` | 436 | `8e6c4306a655ec5136b80178fe01f6a08889e15914e2a21d30b5e757a90db27d` | Emitted `S` and `M`, then no `X`; the single `LDREX` did not return before the deadman reset |
| 4e | `stage4e-smp-nonshared-mmu-ldrex.img` | 436 | `2863c104deca14006f80a2bdef6a6ab167d40d5d0e766adb67a88e4beb8b374f` | Emitted `S` and `M`, then no `X`; clearing the shareable bit did not make the single `LDREX` return |
| 4f | `stage4f-ldrex-exception-vectors.img` | 600 | `fb3387e2a4293b6c6ab9bc93fbebc6912e40c3939aa2ebff095c738929ce1a0a` | Emitted `SMD`; the single `LDREX` raises a data abort |
| 4g | `stage4g-ldrex-fault-registers.img` | 688 | `1ceeca5b3d95a31334188f722bdaa90f0c0772363ab44c339a8506f07d2a970f` | Emitted `SMD00001008@00108000`; `LDREX` caused a synchronous read-side external abort with an AXI slave error at the exact aligned atomic address |
| 4h | `stage4h-cached-mmu-ldrex.img` | 696 | `cfdd64190dc1df58f07ca90f9d063c61e7af1a5d27fc72840fb76f429b5a89c6` | Emitted `SMX`; cached shared Normal RAM and the data cache made the single `LDREX` complete and read zero |
| 4i | `stage4i-cached-mmu-ldrex-strex.img` | 716 | `0871cd8ebb84e117eb3c7889a8948ca64467a3a2f57257e7d7d2affebf52625b` | Emitted `SML`; one `LDREX`/`STREX` pair succeeded and the stored value read back as one |
| 4j | `stage4j-cached-mmu-rust-atomic.img` | 744 | `7ba35058373a8d2d8f33da7cf83f664054376f3c2b2b1d099b980e07b193e99d` | Emitted `SML`; Rust `AtomicU32::compare_exchange` succeeded on physical hardware |
| 5a | `stage5a-cached-allocator-init.img` | 1,124 | `fbc36044be02f68c8d685696401fb58864767fb4fe0fbca82334084f75ed9aa1` | Emitted `SMI`; allocator initialization succeeded on physical hardware |
| 5b | `stage5b-cached-box-allocation.img` | 1,908 | `5462e80890be8641643e3b7550e1069d28198bf9454bc8bb3398b485d50116ae` | Emitted raw bytes `SM C9`, then reset; disassembly confirms `I` is written before allocation and `A` immediately before a reset, so the immediate reset made this result inconclusive and could discard queued UART bytes |
| 5c | `stage5c-cached-box-allocation-deadman.img` | 1,752 | `5f73cda43c775877f9fb03f95d197846bba642c97203943991ec96047588eb2f` | Emitted `SMIA`, then the five-second deadman returned to the loader; allocator initialization and one `Box<u32>` allocation passed physically |
| 6a | `stage6a-full-uart-development-kernel.img` | 110,264 | `c89dbec823ffced86a1d2eab82fd8b5cd8b5c7b924e24c7dfed2835ddd30e77c` | Booted physically through `smp enabled`, `memory mapped`, `heap allocated`, `timer advanced`, and `GABIX USER READY`; accepted the 4,552-byte user ELF with CRC and `USER UPLOAD OK`, then stalled before its first syscall |
| 6b | `stage6b-noncacheable-user-pages.img` | 110,256 | `db862d2c3393f0c00ade6f1bc4704d86288155808fffe68c54713fcd2f89d5df` | QEMU passed two user reloads; changes only dynamically loaded user code and stack sections to Normal non-cacheable memory while retaining cached shared kernel RAM; superseded for physical testing by Stage 6d's recovery instrumentation |
| 6c-QEMU-failed | `stage6c-user-pages-with-deadman.img` | 110,760 | `bfb41447e746a0583aa1f04a3efaf175547ee42547e02e56be257f3a83e7885e` | QEMU exited immediately after `USER UPLOAD OK` because its Pi watchdog model did not tolerate the physical deadman sequence |
| 6d | `stage6d-noncacheable-user-deadman.img` | 110,760 | `e1c60917376e0e3570e68c2f2530f8a76d63efeb7185fd0321dbed339705adc7` | Booted physically to `GABIX USER READY`; accepted the user ELF and printed `USER UPLOAD OK`, then produced no user syscall output; the 10-second deadman successfully returned to `GABIX LOADER READY`, proving non-cacheable user pages alone did not fix execution |
| 6e | `stage6e-user-fault-registers.img` | 110,928 | `3aaec2407133e6e051f2ecb7b65a9af83ea3c31ecce4ab4e97b389372ea5a3c4` | Booted physically to `GABIX USER READY`; the user ELF produced `P0000000D@41000020`, a section permission prefetch abort at its exact entry address, then the deadman returned to `GABIX LOADER READY` |
| 6f | `stage6f-clean-user-descriptor.img` | 110,940 | `0241035587b39e9d05635de9f5d22da61116be42d820a8c8fb7c7240a2aee928` | **Passing physical baseline.** Cleans each changed user translation-table descriptor to RAM before the barrier and TLB invalidation. Booted to `GABIX USER READY`; the same ELF then printed `gabix arm userspace wrote`, exited, and returned to `GABIX USER READY` twice without a reboot |

The Stage 6 user fixture is preserved as `stage6-user-fixture.elf` (4,552
bytes, SHA-256
`a7f4fba0a80792fc8967e7b2f6c6b6f0ffb876ba6d07cbdd879ef76f8ab1d1f7`).

Stage 6e proved the processor's translation-table walk still observed the old
privileged-only section descriptor. `BOOT_TABLE` lives in cached kernel RAM,
and `allow_user_section()` updated the cached copy without cleaning its cache
line. A data synchronization barrier and Translation Lookaside Buffer (TLB)
invalidation cannot expose a dirty cache line to a non-cacheable table walk.
Stage 6f adds the required `DCCMVAC` clean for the changed descriptor before
those operations.

The exact physical logs are preserved as:

- `stage6e-physical-user-fault.log`
- `stage6f-physical-kernel-boot.log`
- `stage6f-physical-user-run1.log`
- `stage6f-physical-user-run2.log`

## Stage 7: autonomous reboot and kernel reload

The preserved `stage7-reboot-user-fixture.elf` is 4,560 bytes with SHA-256
`db116882ed56bee543a57f25a66f6a15a749faf7a725300c35e5d3d5dcde74ca`.
It writes a reboot marker and invokes syscall 169. On the physical Pi 2,
Stage 6f accepted this ELF and the syscall reset the board to
`GABIX LOADER READY`. The host then uploaded the unchanged Stage 6f kernel;
the loader validated its CRC, transferred control to `0x00100000`, and the
kernel returned to `GABIX USER READY`. This lifecycle required no physical
power cycle and no SD-card write.

The reboot happened while the marker remained in the PL011 transmit FIFO, so
the captured marker ends partway through `gabix arm userspace rebooting`. The
complete post-reset loader marker and successful kernel reload establish the
reset result. Exact logs:

- `stage7-physical-reboot.log`
- `stage7-physical-kernel-reload.log`

Stage 0 through Stage 2 passed on the physical Raspberry Pi 2B on 2026-08-16.
