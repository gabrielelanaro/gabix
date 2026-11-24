//! Decode 32-bit, little-endian ARM executable files without allocating memory.
//! The caller supplies the memory mapping and copying through load_segments.
//! Layout: System V ELF ABI, ELF Header and Program Header Entry (Elf32).

mod header {
    pub const BYTES: usize = 52;
    pub const CLASS: usize = 4;
    pub const BYTE_ORDER: usize = 5;
    pub const FILE_TYPE: usize = 16;
    pub const MACHINE: usize = 18;
    pub const ENTRY_ADDRESS: usize = 24;
    pub const PROGRAM_TABLE_OFFSET: usize = 28;
    pub const PROGRAM_ENTRY_BYTES: usize = 42;
    pub const PROGRAM_ENTRY_COUNT: usize = 44;
}

mod program_header {
    pub const BYTES: usize = 32;
    pub const SEGMENT_TYPE: usize = 0;
    pub const FILE_OFFSET: usize = 4;
    pub const VIRTUAL_ADDRESS: usize = 8;
    pub const FILE_BYTES: usize = 16;
    pub const MEMORY_BYTES: usize = 20;
    pub const FLAGS: usize = 24;
}

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const ELF32_CLASS: u8 = 1;
const LITTLE_ENDIAN: u8 = 1;
const EXECUTABLE: u16 = 2;
const ARM_MACHINE: u16 = 40;
const LOAD_SEGMENT: u32 = 1;

pub struct Elf32<'a> {
    bytes: &'a [u8],
    entry: u32,
    program_offset: usize,
    program_size: usize,
    program_count: usize,
}

/// A borrowed file slice plus its requested location and total size in memory.
/// The loader must zero any memory beyond data.len() up to memory_size.
pub struct LoadSegment<'a> {
    pub virtual_address: u32,
    pub memory_size: u32,
    pub flags: u32,
    pub data: &'a [u8],
}

impl<'a> Elf32<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < header::BYTES
            || !bytes[..ELF_MAGIC.len()]
                .iter()
                .zip(ELF_MAGIC)
                .all(|(left, right)| left == right)
            || bytes[header::CLASS] != ELF32_CLASS
            || bytes[header::BYTE_ORDER] != LITTLE_ENDIAN
            || read_u16(bytes, header::FILE_TYPE) != EXECUTABLE
            || read_u16(bytes, header::MACHINE) != ARM_MACHINE
        {
            return None;
        }

        let program_offset = read_u32(bytes, header::PROGRAM_TABLE_OFFSET) as usize;
        let program_size = read_u16(bytes, header::PROGRAM_ENTRY_BYTES) as usize;
        let program_count = read_u16(bytes, header::PROGRAM_ENTRY_COUNT) as usize;
        if program_size < program_header::BYTES
            || program_offset.checked_add(program_size * program_count)? > bytes.len()
        {
            return None;
        }

        Some(Self {
            bytes,
            entry: read_u32(bytes, header::ENTRY_ADDRESS),
            program_offset,
            program_size,
            program_count,
        })
    }

    pub fn entry(&self) -> u32 {
        self.entry
    }

    /// Visit loadable entries in file order. A false callback result stops the
    /// walk; previous callback effects are retained. Address/permission policy
    /// belongs to the caller; this parser checks the segment's file bounds.
    pub fn load_segments(&self, mut load: impl FnMut(LoadSegment<'a>) -> bool) -> bool {
        for index in 0..self.program_count {
            let header = self.program_offset + index * self.program_size;
            if read_u32(self.bytes, header + program_header::SEGMENT_TYPE) != LOAD_SEGMENT {
                continue;
            }

            let file_offset = read_u32(self.bytes, header + program_header::FILE_OFFSET) as usize;
            let virtual_address = read_u32(self.bytes, header + program_header::VIRTUAL_ADDRESS);
            let file_size = read_u32(self.bytes, header + program_header::FILE_BYTES) as usize;
            let memory_size = read_u32(self.bytes, header + program_header::MEMORY_BYTES);
            let flags = read_u32(self.bytes, header + program_header::FLAGS);
            let Some(file_end) = file_offset.checked_add(file_size) else {
                return false;
            };
            if file_end > self.bytes.len() || file_size > memory_size as usize {
                return false;
            }

            if !load(LoadSegment {
                virtual_address,
                memory_size,
                flags,
                data: &self.bytes[file_offset..file_end],
            }) {
                return false;
            }
        }
        true
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
