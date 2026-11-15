use gabix_kernel::device::{BlockDevice, BlockError, WritableBlockDevice};

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
const TIMEOUT_CONTROL: usize = 0x2e;
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
const IDENTIFICATION_CLOCK_HZ: u32 = 400_000;
const TRANSFER_CLOCK_HZ: u32 = 25_000_000;

const INTERRUPT_COMMAND_COMPLETE: u32 = 1 << 0;
const INTERRUPT_TRANSFER_COMPLETE: u32 = 1 << 1;
const INTERRUPT_BUFFER_WRITE_READY: u32 = 1 << 4;
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

pub struct SdCard {
    high_capacity: bool,
    relative_address: u32,
}

impl SdCard {
    pub unsafe fn initialize() -> Option<Self> {
        let source_clock_hz = super::mailbox::emmc_clock_rate()?;
        write8(SOFTWARE_RESET, 1);
        wait_until(|| read8(SOFTWARE_RESET) & 1 == 0)?;
        write32(INTERRUPT_ENABLE, u32::MAX);
        write32(SIGNAL_ENABLE, 0);
        write8(POWER_CONTROL, 0x0f);
        write8(TIMEOUT_CONTROL, 0x0e); // Data timeout: 2^27 timeout-clock cycles.
        set_clock(source_clock_hz, IDENTIFICATION_CLOCK_HZ)?;
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
        set_clock(source_clock_hz, TRANSFER_CLOCK_HZ)?;
        Some(Self {
            high_capacity,
            relative_address,
        })
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

impl WritableBlockDevice for SdCard {
    fn write_sector(&mut self, sector: u64, data: &[u8; 512]) -> Result<(), BlockError> {
        let sector = u32::try_from(sector).map_err(|_| BlockError::AddressOutOfRange)?;
        // SDHC/SDXC cards take a sector number; standard-capacity cards take bytes.
        let argument = if self.high_capacity {
            sector
        } else {
            sector
                .checked_mul(512)
                .ok_or(BlockError::AddressOutOfRange)?
        };
        unsafe {
            wait_data_idle()?;
            write16(BLOCK_SIZE, 512);
            write16(BLOCK_COUNT, 1);
            write32(INTERRUPT_STATUS, u32::MAX);
            write32(ARGUMENT, argument);
            write16(TRANSFER_MODE, 0); // Single-block, host-to-card, programmed I/O.
            write16(
                COMMAND,
                (24 << 8) | RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX | COMMAND_DATA,
            );
            wait_interrupt_result(INTERRUPT_COMMAND_COMPLETE)?;
            check_card_status(read32(RESPONSE_0))?;

            // The controller requests one block. Each FIFO write supplies 4 bytes.
            wait_interrupt_result(INTERRUPT_BUFFER_WRITE_READY)?;
            for bytes in data.chunks_exact(4) {
                write32(BUFFER_DATA, u32::from_le_bytes(bytes.try_into().unwrap()));
            }
            wait_interrupt_result(INTERRUPT_TRANSFER_COMPLETE)?;
            wait_data_idle()?;

            // CMD13 confirms the card finished programming and returned to
            // transfer state before we report success to the caller.
            let started = microseconds();
            loop {
                write32(INTERRUPT_STATUS, u32::MAX);
                write32(ARGUMENT, self.relative_address);
                write16(TRANSFER_MODE, 0);
                write16(
                    COMMAND,
                    (13 << 8) | RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX,
                );
                wait_interrupt_result(INTERRUPT_COMMAND_COMPLETE)?;
                let status = read32(RESPONSE_0);
                check_card_status(status)?;
                let ready_for_data = status & (1 << 8) != 0;
                let transfer_state = (status >> 9) & 0xf == 4;
                if ready_for_data && transfer_state {
                    return Ok(());
                }
                if microseconds().wrapping_sub(started) >= IO_TIMEOUT_US {
                    return Err(BlockError::Timeout);
                }
                wait_data_idle()?;
            }
        }
    }
}

// SD physical-layer R1: address/block/erase/write-protect/lock/CRC/command
// errors (31..19), CSD overwrite (16), WP erase skip (15), AKE sequence (3).
// Reserved bits 18..17 and informational status bits are excluded.
const CARD_ERROR_MASK: u32 = (0x1fff << 19) | (1 << 16) | (1 << 15) | (1 << 3);
const IO_TIMEOUT_US: u32 = 1_000_000;

fn check_card_status(status: u32) -> Result<(), BlockError> {
    if status & CARD_ERROR_MASK != 0 {
        Err(BlockError::Card(status))
    } else {
        Ok(())
    }
}

unsafe fn microseconds() -> u32 {
    core::ptr::read_volatile(0x3f00_3004 as *const u32)
}

unsafe fn wait_data_idle() -> Result<(), BlockError> {
    let started = microseconds();
    while read32(PRESENT_STATE) & (COMMAND_INHIBIT | DATA_INHIBIT) != 0 {
        if microseconds().wrapping_sub(started) >= IO_TIMEOUT_US {
            return Err(BlockError::Timeout);
        }
        core::hint::spin_loop();
    }
    Ok(())
}

unsafe fn wait_interrupt_result(mask: u32) -> Result<(), BlockError> {
    let started = microseconds();
    loop {
        let status = read32(INTERRUPT_STATUS);
        if status & INTERRUPT_ERROR != 0 {
            write32(INTERRUPT_STATUS, status);
            return Err(BlockError::Controller(status));
        }
        if status & mask != 0 {
            write32(INTERRUPT_STATUS, mask);
            return Ok(());
        }
        if microseconds().wrapping_sub(started) >= IO_TIMEOUT_US {
            return Err(BlockError::Timeout);
        }
        core::hint::spin_loop();
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

unsafe fn set_clock(source_hz: u32, maximum_hz: u32) -> Option<()> {
    // Divided mode uses source / (2 * divider). Round up so the SD clock
    // never exceeds the requested rate; encode its ten bits across CONTROL1.
    // Default-speed cards permit up to 25 MHz; initialization permits 400 kHz.
    let divider = source_hz.div_ceil(maximum_hz.checked_mul(2)?).max(1);
    if divider > 0x3ff {
        return None;
    }
    let divider = (((divider & 0xff) << 8) | ((divider >> 8) << 6)) as u16;
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
    wait_interrupt_result(mask).ok()
}

unsafe fn wait_until(mut condition: impl FnMut() -> bool) -> Option<()> {
    let started = microseconds();
    while !condition() {
        if microseconds().wrapping_sub(started) >= IO_TIMEOUT_US {
            return None;
        }
        core::hint::spin_loop();
    }
    Some(())
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
