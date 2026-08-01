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

pub struct LoadSegment<'a> {
    pub virtual_address: u32,
    pub memory_size: u32,
    pub flags: u32,
    pub data: &'a [u8],
}

impl<'a> Elf32<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < 52
            || !bytes[..4]
                .iter()
                .zip(ELF_MAGIC)
                .all(|(left, right)| left == right)
            || bytes[4] != ELF32_CLASS
            || bytes[5] != LITTLE_ENDIAN
            || read_u16(bytes, 16) != EXECUTABLE
            || read_u16(bytes, 18) != ARM_MACHINE
        {
            return None;
        }

        let program_offset = read_u32(bytes, 28) as usize;
        let program_size = read_u16(bytes, 42) as usize;
        let program_count = read_u16(bytes, 44) as usize;
        if program_size < 32
            || program_offset.checked_add(program_size * program_count)? > bytes.len()
        {
            return None;
        }

        Some(Self {
            bytes,
            entry: read_u32(bytes, 24),
            program_offset,
            program_size,
            program_count,
        })
    }

    pub fn entry(&self) -> u32 {
        self.entry
    }

    pub fn load_segments(&self, mut load: impl FnMut(LoadSegment<'a>) -> bool) -> bool {
        for index in 0..self.program_count {
            let header = self.program_offset + index * self.program_size;
            if read_u32(self.bytes, header) != LOAD_SEGMENT {
                continue;
            }

            let file_offset = read_u32(self.bytes, header + 4) as usize;
            let virtual_address = read_u32(self.bytes, header + 8);
            let file_size = read_u32(self.bytes, header + 16) as usize;
            let memory_size = read_u32(self.bytes, header + 20);
            let flags = read_u32(self.bytes, header + 24);
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
