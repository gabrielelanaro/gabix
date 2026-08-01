use gabix_kernel::device::BlockDevice;

const BASE: usize = 0x3f30_0000;
const BLOCK_SIZE: usize = 0x04;
const BLOCK_COUNT: usize = 0x06;
const ARGUMENT: usize = 0x08;
const TRANSFER_MODE: usize = 0x0c;
const COMMAND: usize = 0x0e;
const RESPONSE_0: usize = 0x10;
const BUFFER_DATA: usize = 0x20;
const PRESENT_STATE: usize = 0x24;
const POWER_CONTROL: usize = 0x29;
const CLOCK_CONTROL: usize = 0x2c;
const SOFTWARE_RESET: usize = 0x2f;
const INTERRUPT_STATUS: usize = 0x30;
const INTERRUPT_ENABLE: usize = 0x34;
const SIGNAL_ENABLE: usize = 0x38;

const COMMAND_INHIBIT: u32 = 1 << 0;
const DATA_INHIBIT: u32 = 1 << 1;
const CARD_INSERTED: u32 = 1 << 16;

const CLOCK_INTERNAL_ENABLE: u16 = 1 << 0;
const CLOCK_INTERNAL_STABLE: u16 = 1 << 1;
const CLOCK_SD_ENABLE: u16 = 1 << 2;
const IDENTIFICATION_DIVIDER: u16 = 0x8000;
const TRANSFER_DIVIDER: u16 = 0x0100;

const INTERRUPT_COMMAND_COMPLETE: u32 = 1 << 0;
const INTERRUPT_TRANSFER_COMPLETE: u32 = 1 << 1;
const INTERRUPT_BUFFER_READ_READY: u32 = 1 << 5;
const INTERRUPT_ERROR: u32 = 1 << 15;

const RESPONSE_NONE: u16 = 0;
const RESPONSE_LONG: u16 = 1;
const RESPONSE_48: u16 = 2;
const RESPONSE_48_BUSY: u16 = 3;
const COMMAND_CRC: u16 = 1 << 3;
const COMMAND_INDEX: u16 = 1 << 4;
const COMMAND_DATA: u16 = 1 << 5;
const TRANSFER_READ: u16 = 1 << 4;

const OCR_VOLTAGE: u32 = 0x00ff_8000;
const OCR_HIGH_CAPACITY: u32 = 1 << 30;
const OCR_READY: u32 = 1 << 31;
const POLL_LIMIT: usize = 100_000;

pub struct SdCard {
    high_capacity: bool,
}

impl SdCard {
    pub unsafe fn initialize() -> Option<Self> {
        write8(SOFTWARE_RESET, 1);
        wait_until(|| read8(SOFTWARE_RESET) & 1 == 0)?;
        write32(INTERRUPT_ENABLE, u32::MAX);
        write32(SIGNAL_ENABLE, 0);
        write8(POWER_CONTROL, 0x0f);
        set_clock(IDENTIFICATION_DIVIDER)?;
        wait_until(|| read32(PRESENT_STATE) & CARD_INSERTED != 0)?;

        command(0, 0, RESPONSE_NONE)?;
        let _ = command(8, 0x1aa, RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX)?;

        let mut ocr = 0;
        for _ in 0..1000 {
            command(55, 0, RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX)?;
            ocr = command(41, OCR_VOLTAGE | OCR_HIGH_CAPACITY, RESPONSE_48)?;
            if ocr & OCR_READY != 0 {
                break;
            }
        }
        if ocr & OCR_READY == 0 {
            return None;
        }

        command(2, 0, RESPONSE_LONG | COMMAND_CRC)?;
        let relative_address =
            command(3, 0, RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX)? & 0xffff_0000;
        if relative_address == 0 {
            return None;
        }
        command(
            7,
            relative_address,
            RESPONSE_48_BUSY | COMMAND_CRC | COMMAND_INDEX,
        )?;
        wait_until(|| read32(PRESENT_STATE) & DATA_INHIBIT == 0)?;

        let high_capacity = ocr & OCR_HIGH_CAPACITY != 0;
        if !high_capacity {
            command(16, 512, RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX)?;
        }
        set_clock(TRANSFER_DIVIDER)?;
        Some(Self { high_capacity })
    }

    pub unsafe fn try_read_sector(&mut self, sector: u64) -> Option<[u8; 512]> {
        let sector = u32::try_from(sector).ok()?;
        let argument = if self.high_capacity {
            sector
        } else {
            sector.checked_mul(512)?
        };
        wait_until(|| read32(PRESENT_STATE) & (COMMAND_INHIBIT | DATA_INHIBIT) == 0)?;
        write16(BLOCK_SIZE, 512);
        write16(BLOCK_COUNT, 1);
        write32(INTERRUPT_STATUS, u32::MAX);
        write32(ARGUMENT, argument);
        write16(TRANSFER_MODE, TRANSFER_READ);
        write16(
            COMMAND,
            (17 << 8) | RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX | COMMAND_DATA,
        );
        wait_interrupt(INTERRUPT_COMMAND_COMPLETE)?;
        wait_interrupt(INTERRUPT_BUFFER_READ_READY)?;

        let mut sector = [0u8; 512];
        for chunk in sector.chunks_exact_mut(4) {
            chunk.copy_from_slice(&read32(BUFFER_DATA).to_le_bytes());
        }
        wait_interrupt(INTERRUPT_TRANSFER_COMPLETE)?;
        Some(sector)
    }
}

unsafe impl Send for SdCard {}

impl BlockDevice for SdCard {
    fn read_sector(&mut self, sector: u64) -> [u8; 512] {
        unsafe { self.try_read_sector(sector).unwrap_or([0; 512]) }
    }
}

unsafe fn command(index: u16, argument: u32, flags: u16) -> Option<u32> {
    let inhibit = if flags & COMMAND_DATA != 0 {
        COMMAND_INHIBIT | DATA_INHIBIT
    } else {
        COMMAND_INHIBIT
    };
    wait_until(|| read32(PRESENT_STATE) & inhibit == 0)?;
    write32(INTERRUPT_STATUS, u32::MAX);
    write32(ARGUMENT, argument);
    write16(TRANSFER_MODE, 0);
    write16(COMMAND, (index << 8) | flags);
    wait_interrupt(INTERRUPT_COMMAND_COMPLETE)?;
    Some(read32(RESPONSE_0))
}

unsafe fn set_clock(divider: u16) -> Option<()> {
    write16(CLOCK_CONTROL, 0);
    write16(CLOCK_CONTROL, divider | CLOCK_INTERNAL_ENABLE);
    wait_until(|| read16(CLOCK_CONTROL) & CLOCK_INTERNAL_STABLE != 0)?;
    write16(
        CLOCK_CONTROL,
        divider | CLOCK_INTERNAL_ENABLE | CLOCK_SD_ENABLE,
    );
    Some(())
}

unsafe fn wait_interrupt(mask: u32) -> Option<()> {
    for _ in 0..POLL_LIMIT {
        let status = read32(INTERRUPT_STATUS);
        if status & INTERRUPT_ERROR != 0 {
            write32(INTERRUPT_STATUS, status);
            return None;
        }
        if status & mask != 0 {
            write32(INTERRUPT_STATUS, mask);
            return Some(());
        }
        core::hint::spin_loop();
    }
    None
}

unsafe fn wait_until(mut condition: impl FnMut() -> bool) -> Option<()> {
    for _ in 0..POLL_LIMIT {
        if condition() {
            return Some(());
        }
        core::hint::spin_loop();
    }
    None
}

unsafe fn read8(offset: usize) -> u8 {
    core::ptr::read_volatile((BASE + offset) as *const u8)
}

unsafe fn read16(offset: usize) -> u16 {
    core::ptr::read_volatile((BASE + offset) as *const u16)
}

unsafe fn read32(offset: usize) -> u32 {
    core::ptr::read_volatile((BASE + offset) as *const u32)
}

unsafe fn write8(offset: usize, value: u8) {
    core::ptr::write_volatile((BASE + offset) as *mut u8, value);
}

unsafe fn write16(offset: usize, value: u16) {
    core::ptr::write_volatile((BASE + offset) as *mut u16, value);
}

unsafe fn write32(offset: usize, value: u32) {
    core::ptr::write_volatile((BASE + offset) as *mut u32, value);
}
