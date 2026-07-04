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
        let u16_at = |off: usize| -> u16 {
            u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap())
        };
        let u32_at = |off: usize| -> u32 {
            u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap())
        };
        let u64_at = |off: usize| -> u64 {
            u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap())
        };

        Some(Self {
            // e_ident sub-bytes
            class:   bytes[4],
            data:    bytes[5],
            version: bytes[6],
            osabi:   bytes[7],
            // Main header
            e_type:     u16_at(0x10),
            machine:    u16_at(0x12),
            entry:      u64_at(0x18),
            phoff:      u64_at(0x20),
            shoff:      u64_at(0x28),
            flags:      u32_at(0x30),
            ehsize:     u16_at(0x34),
            phentsize:  u16_at(0x36),
            phnum:      u16_at(0x38),
            shentsize:  u16_at(0x3A),
            shnum:      u16_at(0x3C),
            shstrndx:   u16_at(0x3E),
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
        println!("[elf]   class        = {} ({}-bit)",
            self.class, if self.class == ELFCLASS64 { 64 } else { 32 });
        println!("[elf]   data         = {} ({})",
            self.data, if self.data == ELFDATA2LSB { "little-endian" } else { "big-endian" });
        println!("[elf]   osabi        = {}", self.osabi);
        println!("[elf]   e_type       = {} ({})",
            self.e_type,
            match self.e_type {
                ET_EXEC => "ET_EXEC",
                ET_DYN  => "ET_DYN",
                _       => "?",
            });
        println!("[elf]   machine      = 0x{:X} ({})",
            self.machine,
            if self.machine == EM_X86_64 { "x86-64" } else { "?" });
        println!("[elf]   entry        = 0x{:X}", self.entry);
        println!("[elf]   phoff        = {}", self.phoff);
        println!("[elf]   phentsize    = {}", self.phentsize);
        println!("[elf]   phnum        = {}", self.phnum);
        println!("[elf]   is_x86_64_exec = {}", self.is_x86_64_exec());
    }
}
