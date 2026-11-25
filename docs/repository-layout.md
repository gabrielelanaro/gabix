# Gabix repository layout

**Proposed architecture.** Make both kernel binaries visible as sibling Cargo packages. Make the root manifest describe the workspace. A reader opening the repository should be able to locate the x86 kernel, the ARM kernel, shared kernel code, board experiments, and the programs that run on the host.

## Current ownership and proposed destinations

The HTML map opens on the actual working tree. The destinations below describe proposed changes. The directory inventory includes the individual paths underneath these groups.

| Current location | Proposed location | What belongs there |
|---|---|---|
| Root `Cargo.toml`, `src/main.rs` | Root workspace manifest and `targets/x86_64/{Cargo.toml,src/main.rs}` | The root enumerates members. The x86 package declares its bootloader, CPU, interrupt-controller, and emulator dependencies. |
| `src/arch/x86_64/`, `src/drivers/`, x86 console and boot support | `targets/x86_64/src/` | The x86 package owns register frames, page tables, interrupt setup, and its current PCI/virtio drivers. Keep existing internal paths during the package move. |
| `src/{fs,net,syscall,task}/` and related root kernel modules | Initially `targets/x86_64/src/`; move reusable behavior into `crates/kernel-core/src/` as both callers adopt it | Keep target-specific syscall entry, user-memory access, locks, and scheduler integration in the target. Shared modules can own on-disk encodings, TCP state transitions, and descriptor-slot operations. A filename alone cannot establish portability. |
| `targets/armv7/src/`, including `raspi2b/` and `virt/` | Same target package and board directories | ARM exception entry, MMU setup, context switching, board initialization, and device adapters. Keep QEMU virt and Raspberry Pi as two boards using the ARM package. |
| Standalone probe binaries in `targets/armv7/src/`, diagnostic binaries in `targets/armv7/tests/`, existing `targets/armv7/probes/` | `targets/armv7/probes/` for hardware probes and SD/Wi-Fi checks; `targets/armv7/fixtures/boot/` for the loader-trial image | Group each standalone binary with its assembly and linker inputs. Preserve the difference between bootable diagnostic images and callable RAM probes. Keep an assembly fragment used directly by a running kernel beside its owner. |
| `crates/kernel-core/src/`, `crates/kernel-core/tests/` | Retain the shared crate and its host tests | Both targets depend on this crate. Device traits and pure protocol rules can live here. Give chip-specific packet formats a driver-owned module even when their encoding can be tested on the host. |
| `tests/`, `tests/fixtures/`, `tests/support/` | Repository-level behavior tests, fixture programs, and test-only helpers | QEMU and physical-board drivers belong here. A host test for one package can live under the owning package. The general ARM userspace linker script belongs with the ARM target because normal build commands consume it. |
| `scripts/` | Keep documented host commands; group probe builders and reusable host code by purpose | Keep existing command paths working when moving implementation. For repeated probe wrappers, share the Cargo/objcopy steps while keeping per-probe link address, features, and binary name explicit. |
| `docs/`, `docs/adr/`, `artifacts/` | Engineering documents, individually numbered decisions, and preserved hardware evidence | Give each accepted decision a distinct ID. Keep recovery image metadata and concise hardware findings in the artifact locations defined by project policy. |
| `lessons/`, `tours/`, `reference/`, `assets/`, `learning-records/` | Keep course content, navigation, shared assets, and learning records under their existing public paths | These paths have readers and links independent of Cargo. Review a relocation together with its incoming links. |
| Root project guidance, Cargo/toolchain metadata, agent configuration and vendored skills | Keep files at their loader's discovery locations; identify vendored trees separately | Include their size and paths in the inventory. Apply kernel placement rules to the owned kernel packages. |

## Why the workspace move comes first

The root [package](../Cargo.toml) declares x86 dependencies. The root [Cargo configuration](../.cargo/config.toml) selects the x86 JSON target and places the x86 image base and boot runner under a selector matching bare-metal targets. The [ARM build](../scripts/build-arm.sh) selects its own manifest, target triple, linker script, and flags. The map therefore shows one directory named after a target even though two kernels build.

1. **Make the package roots symmetric.** Move the existing root kernel package into `targets/x86_64/` together with its x86 specification. Retain the package name `mini-os` for bootimage consumers. Make the root a virtual workspace containing both targets and kernel-core. Preserve lockfile resolution and toolchain settings. Use explicit build targets for x86, ARM, and host tests. Cargo config lookup depends on the invocation directory; merely relocating a config beside a manifest does not verify a build from the root.
2. **Prove the package move without changing kernel behavior.** Exercise the documented root invocation, [x86 boot helper](../tests/common.sh), [ARM builder](../scripts/build-arm.sh), and a host kernel-core test. Verify the emitted kernel image paths and boot each QEMU target. Keep board upload and recovery image commands working. A physical install is a separate change from moving a package.
3. **Separate diagnostics from runtime code.** Use Cargo binary declarations to identify probe entry points. Update assembly includes, Rust module paths, linker scripts, and shell callers together. Build a standalone boot probe, a callable RAM probe, and both kernels. The existing [LAN9512 probe](../targets/armv7/src/pi2_lan9512_receive_probe.rs) demonstrates why moving a directory alone breaks imports.
4. **Then extract shared behavior one operation at a time.** Preserve the old x86 behavior with tests where appropriate, specify deliberate differences such as the bytes returned for an ext2 sparse hole, and switch both callers. Extracting policy after both package roots are explicit makes each dependency visible in review.

## Reproduce the audit

From the checkout, run these commands with Python 3.11 or newer:

```sh
python3 scripts/audit-layout.py --output target/layout-audit.json
python3 scripts/build-architecture-map.py
python3 tests/test_layout_audit.py
```

Open `target/architecture-map.html` for the current tree. Use the JSON inventory for scripts and comparison between revisions. Rules use path roles, build declarations, source references, or content matches. A rule finding identifies a conflict with the proposed layout. A review entry marks a decision the checker could not settle. A covered entry means the configured checks completed for that path. The classification is relative to these published rules.

Cross-file checks group duplicate numeric ADR IDs and compare implementation bytes across probe and production directories. They assign the finding to the excess entry so a directory total can be traced back to individual records. The LOC area and the placement count measure different things: a one-line manifest can explain a repository-wide layout discrepancy.
