# Lesson 15 complete — ELF header parser works

User completed Lesson 15 (all four steps 15.1 – 15.4) in a single
session. The kernel can now read an ELF binary from disk via VFS,
parse its 64-byte header and 56-byte program headers, and verify
the structure is loadable. One step from the mission goal.

**What was built (4 commits):**

| Step | Commit | What landed |
|---|---|---|
| 15.1 | `fb36235` | `make-disk.sh` cross-compiles a 4-line `x86_64-unknown-none` binary via `rustc -C relocation-model=static` and writes it to `/hello.elf` via debugfs |
| 15.2 | `8403ef8` | `src/elf.rs` (new): `ElfHeader` struct, `parse()`, `is_x86_64_exec()`. Constants for ELF_MAGIC, ELFCLASS64, ELFDATA2LSB, ET_EXEC, EM_X86_64 |
| 15.3 | `414704d` | `ProgramHeader` struct, `parse()`, `is_load()`, `perms_str()`, `type_str()`. Constants for PT_LOAD/PT_DYNAMIC/PT_INTERP/PT_NOTE/PT_PHDR/PT_GNU_STACK + PF_R/PF_W/PF_X |
| 15.4 | `f480577` | `main.rs` Lesson 15 test block: resolve /hello.elf via VFS, parse, validate, walk PHT, confirm entry inside an r-x PT_LOAD |

**Key technical landings:**

- **ELF exists because code can't be relocated.** Addresses are baked into the instruction bytes themselves — `mov rax, [0x402000]` is 7 bytes where the last 4 encode `0x402000`. If the kernel doesn't put the variable at exactly that virtual address, the instruction reads garbage. The file format exists to carry the map.

- **File is range-mapped, not byte-mapped.** Each PT_LOAD entry is `(file_range, vaddr, perms)`, not `{addr: byte}`. The bytes are stored contiguously in the file and copied verbatim into a contiguous range of memory. Same shape as ext2's "these 16 bytes live at sectors 100-102."

- **Segments vs sections.** Two different ways to slice the same file. *Segments* are for the loader — "what bytes go into memory." *Sections* are for the linker and debugger — `.text`, `.data`, `.rodata`, `.bss`. The kernel reads segments; sections are ignored at load time. Stripped binaries omit them entirely.

- **PIE vs static.** `rustc --target x86_64-unknown-none` defaults to ET_DYN (position-independent executable). Adding `-C relocation-model=static` produces ET_EXEC (fixed addresses), which lets Lesson 16 skip the dynamic-relocation handling a PIE binary would need.

- **ET_EXEC only, for now.** The parser refuses ET_DYN, ELFCLASS32, big-endian, and non-x86-64 binaries. Real Linux handles all of these; we don't need to.

**Discoveries:**

- **Lesson 14's trait design paid off.** The ELF parser consumes `&[u8]` and doesn't know ext2 or virtio-blk exists. The same parser would work against a RAM disk or a 9p mount. Clean layering.

- **Two PT_LOAD segments in hello.elf, not the four I expected.** The first PT_LOAD covers file bytes 0..0x184 (388 bytes) including the ELF header and PHT themselves — common for static binaries, since the linker lays them out together as part of the read-only region. The second PT_LOAD is just 4 bytes (`_start: jmp .`) at 0x201190. Two segments, both readable; the second also executable.

- **Entry-point validation is non-trivial.** A naive loader would just jump to `e_entry` and hope. The Lesson 15 test asserts `e_entry` falls inside an r-x PT_LOAD segment's `[p_vaddr, p_vaddr + p_memsz)` range. Catches a corrupt binary at load time rather than via a user-mode page fault.

- **`mktemp` template naming matters.** Initial template `/tmp/mini-os-hello-user.XXXXXX.rs` produced filenames like `mini-os-hello-user.abc123.rs`, which `rustc` rejected: "invalid character '.' in crate name." Switched to dashes (`mini-os-hello-user-XXXXXX.rs`). The crate name is derived from the filename stem; dots confuse it.

- **One shot, no build errors.** All four steps compiled cleanly on first try. Only fix needed: bump Lesson 13's directory-walk assertion from 5 to 6 entries (hello.elf joined the ext2 root). Caught by QEMU run, not by reading the code.

**The bigger picture:**

- Lessons 1-3: VFS abstraction (open/read/write/close, FdTable, path resolution).
- Lessons 5-9: disk path (PCI → virtio → virtqueues → block layer).
- Lessons 10-14: filesystem (ext2 superblock → inode → directory → mount via trait).
- Lesson 15: parse a binary file format on top of VFS.
- Lesson 16: map PT_LOAD segments into a fresh `AddressSpace`, jump to `e_entry`. The mission lands.

**Calibration data — Lesson 15 wall clock:**

| Step | Time | Notes |
|---|---|---|
| 15.1 | ~10 min | make-disk.sh update + mktemp naming fix + verify with debugfs |
| 15.2 | ~5 min | ElfHeader struct + parse + helpers |
| 15.3 | ~5 min | ProgramHeader struct + parse + helpers |
| 15.4 | ~10 min | main.rs test block + QEMU run + assertion bump |
| **Total** | **~30 min** | Smoothest lesson since Lesson 9. Conceptual foundation was the hard part — once that landed, the code is mechanical. |

**Sidebars during Lesson 15:**

- **Why an "executable file" is more than raw bytes.** User self-reported not understanding ELF structure. Socratic exchange nailed the foundational intuition: code can't be relocated because addresses are baked into the instruction bytes themselves; data can. The file format carries the map saying "these bytes go to that virtual address, with these permissions."

- **Three categories of bytes in a binary.** Code (`.text` — executable), read-only constants (`.rodata` — strings, lookup tables), writable variables (`.data`). Each needs different page permissions, which is why an ELF has multiple PT_LOAD segments rather than one blob. There's also `.bss` (zero-initialized globals) — bytes that take no file space but appear in memory; Lesson 16 handles those via `p_memsz > p_filesz`.

- **Multi-section explainers are still jargon dumps.** User asked "introduce the concepts I don't know" and I responded with three labeled sections. They pushed back: "you have thrown at me a bunch of shit that I have no clue about." Lesson: even when the user invites a list, pick ONE concept, ignore the rest, end with a probing question. The list comes later.

See [[0015-lesson-13-complete]] for the prior lesson record.
