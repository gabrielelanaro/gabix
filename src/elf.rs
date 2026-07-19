//! ELF (Executable and Linkable Format) parser — Lesson 15.
//!
//! ELF is the file format Linux uses for binaries. The kernel reads
//! the header at offset 0, then walks the Program Header Table (PHT)
//! — an array of 56-byte entries immediately after the 64-byte header.
//! Each entry describes one chunk of the program that needs to land
//! in memory: which file bytes, which virtual address, what size,
//! what permissions.
//!
//! Lesson 15 is parsing only. Lesson 16 will map PT_LOAD segments
//! into a fresh AddressSpace and jump to e_entry — that's exec().
//!
//! Reference: the System V ABI Generic Introduction (gABI) and
//! Linux's elf(5) man page document the field offsets. The kernel's
//! loader lives in fs/binfmt_elf.c (load_elf_binary).

use crate::println;

/// The ELF magic — four bytes at offset 0 of every ELF file.
/// `0x7F` followed by the ASCII for 'E', 'L', 'F'.
pub const ELF_MAGIC: [u8; 4] = [0x7F, 0x45, 0x4C, 0x46];

/// `e_ident[EI_CLASS]` value for 64-bit binaries.
pub const ELFCLASS64: u8 = 2;
/// `e_ident[EI_DATA]` value for little-endian (LSB) binaries.
pub const ELFDATA2LSB: u8 = 1;

/// `e_type` value for executables (fixed addresses, no dynamic linker).
pub const ET_EXEC: u16 = 2;
/// `e_type` value for shared libraries and PIE executables.
pub const ET_DYN: u16 = 3;

/// `e_machine` value for x86-64.
pub const EM_X86_64: u16 = 0x3E;

/// The 64-byte ELF header (Elf64_Ehdr in the spec).
///
/// We model every load-bearing field at its spec-mandated offset.
/// The `e_ident` region (bytes 0..16) is split into individual bytes
/// because its sub-fields are individually meaningful (class, data,
/// version, OS/ABI) and parsing them as a struct would require
/// `repr(C)` padding tricks.
#[derive(Debug)]
pub struct ElfHeader {
    /// `e_ident[EI_CLASS]` — 1 = 32-bit, 2 = 64-bit. Must be 2 for us.
    pub class: u8,
    /// `e_ident[EI_DATA]` — 1 = little-endian, 2 = big-endian. Must be 1.
    pub data: u8,
    /// `e_ident[EI_VERSION]` — always 1.
    pub version: u8,
    /// `e_ident[EI_OSABI]` — 0 = System V, 3 = Linux. We accept either.
    pub osabi: u8,
    /// `e_type` — what kind of ELF (executable, library, ...).
    pub e_type: u16,
    /// `e_machine` — target architecture (0x3E = x86-64).
    pub machine: u16,
    /// `e_entry` — virtual address the CPU jumps to after loading.
    pub entry: u64,
    /// `e_phoff` — file offset of the Program Header Table.
    pub phoff: u64,
    /// `e_shoff` — file offset of the Section Header Table (we ignore).
    pub shoff: u64,
    /// `e_flags` — architecture-specific flags (0 on x86-64).
    pub flags: u32,
    /// `e_ehsize` — size of this header (always 64 for ELF64).
    pub ehsize: u16,
    /// `e_phentsize` — size of one program-header entry (56 for ELF64).
    pub phentsize: u16,
    /// `e_phnum` — how many program-header entries.
    pub phnum: u16,
    /// `e_shentsize` — size of one section-header entry (we ignore).
    pub shentsize: u16,
    /// `e_shnum` — number of section-header entries (we ignore).
    pub shnum: u16,
    /// `e_shstrndx` — section-name string-table index (we ignore).
    pub shstrndx: u16,
}

impl ElfHeader {
    /// Parse the first 64 bytes of an ELF file into an `ElfHeader`.
    ///
    /// Returns `None` if the bytes are too short or the magic doesn't
    /// match. Other field-level validation (class, machine, type) is
    /// the caller's job — see `is_x86_64_exec` below.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 64 {
            return None;
        }
        if &bytes[0..4] != ELF_MAGIC {
            return None;
        }

        // Local helpers — same pattern as ext2.rs. We've already
        // checked len() >= 64, and all offsets below stay in range,
        // so unwrap() never fires in practice.
        let u16_at =
            |off: usize| -> u16 { u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap()) };
        let u32_at =
            |off: usize| -> u32 { u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) };
        let u64_at =
            |off: usize| -> u64 { u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap()) };

        Some(Self {
            // e_ident sub-bytes
            class: bytes[4],
            data: bytes[5],
            version: bytes[6],
            osabi: bytes[7],
            // Main header
            e_type: u16_at(0x10),
            machine: u16_at(0x12),
            entry: u64_at(0x18),
            phoff: u64_at(0x20),
            shoff: u64_at(0x28),
            flags: u32_at(0x30),
            ehsize: u16_at(0x34),
            phentsize: u16_at(0x36),
            phnum: u16_at(0x38),
            shentsize: u16_at(0x3A),
            shnum: u16_at(0x3C),
            shstrndx: u16_at(0x3E),
        })
    }

    /// True if this is a 64-bit, little-endian, ET_EXEC, x86-64 binary.
    ///
    /// These are the four conditions the Lesson 16 loader will require
    /// before attempting to map the binary. ET_DYN (PIE) would need
    /// relocation handling; 32-bit or big-endian is a different ABI.
    pub fn is_x86_64_exec(&self) -> bool {
        self.class == ELFCLASS64
            && self.data == ELFDATA2LSB
            && self.e_type == ET_EXEC
            && self.machine == EM_X86_64
    }

    /// Pretty-print the parsed fields for boot-time verification.
    pub fn print(&self) {
        println!("[elf] parsed ELF header:");
        println!(
            "[elf]   class        = {} ({}-bit)",
            self.class,
            if self.class == ELFCLASS64 { 64 } else { 32 }
        );
        println!(
            "[elf]   data         = {} ({})",
            self.data,
            if self.data == ELFDATA2LSB {
                "little-endian"
            } else {
                "big-endian"
            }
        );
        println!("[elf]   osabi        = {}", self.osabi);
        println!(
            "[elf]   e_type       = {} ({})",
            self.e_type,
            match self.e_type {
                ET_EXEC => "ET_EXEC",
                ET_DYN => "ET_DYN",
                _ => "?",
            }
        );
        println!(
            "[elf]   machine      = 0x{:X} ({})",
            self.machine,
            if self.machine == EM_X86_64 {
                "x86-64"
            } else {
                "?"
            }
        );
        println!("[elf]   entry        = 0x{:X}", self.entry);
        println!("[elf]   phoff        = {}", self.phoff);
        println!("[elf]   phentsize    = {}", self.phentsize);
        println!("[elf]   phnum        = {}", self.phnum);
        println!("[elf]   is_x86_64_exec = {}", self.is_x86_64_exec());
    }
}

// ============================================================
// Program Header (Elf64_Phdr) — one entry in the PHT
// ============================================================

/// `p_type` values we recognize. Only PT_LOAD causes the loader to
/// copy bytes into memory; the others are hints the kernel reads and
/// discards. The values are part of the ELF spec, not Linux-specific.
pub const PT_LOAD: u32 = 1; // Loadable segment — copy into memory.
pub const PT_DYNAMIC: u32 = 2; // Dynamic linking info (for ld.so).
pub const PT_INTERP: u32 = 3; // Path to the dynamic linker.
pub const PT_NOTE: u32 = 4; // Metadata (build-id, ABI properties).
pub const PT_PHDR: u32 = 6; // The PHT itself, as a segment.
/// PT_GNU_STACK — marker for "should the stack be executable?". The
/// weird number is `0x6474e551` — a magic in the OS-specific range.
pub const PT_GNU_STACK: u32 = 0x6474E551;

/// Permission bits for `p_flags`. Maps directly to page-table bits
/// in Lesson 16: PF_R → readable, PF_W → writable, PF_X → executable.
pub const PF_R: u32 = 0x4;
pub const PF_W: u32 = 0x2;
pub const PF_X: u32 = 0x1;

/// The 56-byte program-header entry (Elf64_Phdr in the spec).
///
/// One entry per chunk of the program. The kernel reads the whole
/// table (e_phnum entries starting at e_phoff) and processes each:
/// PT_LOAD entries get mapped; others get read and discarded.
#[derive(Debug)]
pub struct ProgramHeader {
    /// `p_type` — what kind of segment this is. PT_LOAD = 1 means
    /// "copy these bytes into memory".
    pub p_type: u32,
    /// `p_flags` — permission bits (PF_R | PF_W | PF_X).
    pub p_flags: u32,
    /// `p_offset` — file offset where this segment's bytes start.
    pub p_offset: u64,
    /// `p_vaddr` — virtual address where these bytes should land.
    pub p_vaddr: u64,
    /// `p_paddr` — physical address (unused under Linux paging).
    pub p_paddr: u64,
    /// `p_filesz` — how many bytes exist in the file.
    pub p_filesz: u64,
    /// `p_memsz` — how many bytes the segment occupies in memory.
    /// If `p_memsz > p_filesz`, the extra bytes are zeroed — that's
    /// how .bss (zero-initialized globals) works.
    pub p_memsz: u64,
    /// `p_align` — alignment of both p_offset and p_vaddr. Usually
    /// the page size (4096) for PT_LOAD.
    pub p_align: u64,
}

impl ProgramHeader {
    /// Parse 56 bytes into a `ProgramHeader`. Returns `None` on
    /// short input. The ELF magic / class / etc. checks happen at
    /// the header level, not per-entry.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 56 {
            return None;
        }

        let u32_at =
            |off: usize| -> u32 { u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) };
        let u64_at =
            |off: usize| -> u64 { u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap()) };

        Some(Self {
            p_type: u32_at(0x00),
            p_flags: u32_at(0x04),
            p_offset: u64_at(0x08),
            p_vaddr: u64_at(0x10),
            p_paddr: u64_at(0x18),
            p_filesz: u64_at(0x20),
            p_memsz: u64_at(0x28),
            p_align: u64_at(0x30),
        })
    }

    /// True if this entry is a loadable segment.
    pub fn is_load(&self) -> bool {
        self.p_type == PT_LOAD
    }

    /// Human-readable permission string ("r-x", "rw-", etc.) for
    /// printing. Three letters, mirrors `ls -l` syntax.
    pub fn perms_str(&self) -> &'static str {
        let r = self.p_flags & PF_R != 0;
        let w = self.p_flags & PF_W != 0;
        let x = self.p_flags & PF_X != 0;
        match (r, w, x) {
            (false, false, false) => "---",
            (false, false, true) => "--x",
            (false, true, false) => "-w-",
            (false, true, true) => "-wx",
            (true, false, false) => "r--",
            (true, false, true) => "r-x",
            (true, true, false) => "rw-",
            (true, true, true) => "rwx",
        }
    }

    /// Short label for `p_type` for printing.
    pub fn type_str(&self) -> &'static str {
        match self.p_type {
            PT_LOAD => "LOAD",
            PT_DYNAMIC => "DYN ",
            PT_INTERP => "INTRP",
            PT_NOTE => "NOTE",
            PT_PHDR => "PHDR",
            PT_GNU_STACK => "GSTK",
            _ => "OTHER",
        }
    }
}
