//! SD Host Controller Interface (SDHCI), using polled programmed I/O.
//! The CPU sends a command and copies words through BUFFER_DATA. Status flags
//! report progress; SIGNAL_ENABLE stays zero, so this path does not use IRQs.
//! Register/command references: Linux drivers/mmc/host/sdhci.h and include/linux/mmc/{sd,mmc}.h.

use gabix_kernel::device::{BlockDevice, BlockError, WritableBlockDevice};

mod card_command {
    pub const GO_IDLE: u16 = 0;
    pub const ALL_SEND_CID: u16 = 2;
    pub const SEND_RELATIVE_ADDRESS: u16 = 3;
    pub const SELECT_CARD: u16 = 7;
    pub const SEND_INTERFACE_CONDITION: u16 = 8;
    pub const SEND_CSD: u16 = 9;
    pub const SEND_STATUS: u16 = 13;
    pub const SET_BLOCK_LENGTH: u16 = 16;
    pub const READ_SINGLE_BLOCK: u16 = 17;
    pub const WRITE_SINGLE_BLOCK: u16 = 24;
    pub const APPLICATION_OPERATING_CONDITION: u16 = 41;
    pub const APPLICATION_PREFIX: u16 = 55;
}

// Card-Specific Data (CSD) field positions after restoring the response shift.
mod csd_field {
    pub const FORMAT_SHIFT: u32 = 126;
    pub const STANDARD_CAPACITY: u128 = 0;
    pub const HIGH_CAPACITY: u128 = 1;
    pub const READ_BLOCK_EXPONENT_SHIFT: u32 = 80;
    pub const READ_BLOCK_EXPONENT_MASK: u128 = 0xf;
    pub const STANDARD_SIZE_SHIFT: u32 = 62;
    pub const STANDARD_SIZE_MASK: u128 = 0xfff;
    pub const SIZE_MULTIPLIER_SHIFT: u32 = 47;
    pub const SIZE_MULTIPLIER_MASK: u128 = 7;
    pub const HIGH_CAPACITY_SIZE_SHIFT: u32 = 48;
    pub const HIGH_CAPACITY_SIZE_MASK: u128 = 0x3f_ffff;
    pub const SECTORS_PER_CAPACITY_UNIT: u64 = 1024; // 512 KiB / 512 bytes.
}

const CLOCK_DIVIDER_MAX: u32 = 0x3ff;
const CLOCK_DIVIDER_LOW_MASK: u32 = 0xff;
const CLOCK_DIVIDER_LOW_BITS: u32 = 8;
const CLOCK_DIVIDER_LOW_REGISTER_SHIFT: u32 = 8;
const CLOCK_DIVIDER_HIGH_REGISTER_SHIFT: u32 = 6;

const RESET_ALL: u8 = 1;
const POWER_ON_3V3: u8 = 0x0f;
const DATA_TIMEOUT_2_POW_27: u8 = 0x0e;
const INITIALIZATION_ATTEMPTS: usize = 1000;
const INTERFACE_CONDITION_3V3_PATTERN: u32 = 0x1aa;
const RELATIVE_ADDRESS_MASK: u32 = 0xffff_0000;
const SECTOR_BYTES: usize = 512;
const DATA_WORD_BYTES: usize = 4;
const COMMAND_NUMBER_SHIFT: u32 = 8;
const TRANSFER_SINGLE_BLOCK_WRITE_PIO: u16 = 0;
const CARD_READY_FOR_DATA: u32 = 1 << 8;
const CARD_STATE_SHIFT: u32 = 9;
const CARD_STATE_MASK: u32 = 0xf;
const CARD_STATE_TRANSFER: u32 = 4;
const SYSTEM_TIMER_LOW: *const u32 = 0x3f00_3004 as *const u32;

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
    sector_count: u64,
}

impl SdCard {
    pub unsafe fn initialize() -> Option<Self> {
        let source_clock_hz = super::mailbox::emmc_clock_rate()?;
        write8(SOFTWARE_RESET, RESET_ALL);
        wait_until(|| read8(SOFTWARE_RESET) & RESET_ALL == 0)?;
        write32(INTERRUPT_ENABLE, u32::MAX);
        write32(SIGNAL_ENABLE, 0);
        write8(POWER_CONTROL, POWER_ON_3V3);
        write8(TIMEOUT_CONTROL, DATA_TIMEOUT_2_POW_27); // Data timeout: 2^27 timeout-clock cycles.
        set_clock(source_clock_hz, IDENTIFICATION_CLOCK_HZ)?;
        wait_until(|| read32(PRESENT_STATE) & CARD_INSERTED != 0)?;

        command(card_command::GO_IDLE, 0, RESPONSE_NONE)?;
        let _ = command(
            card_command::SEND_INTERFACE_CONDITION,
            INTERFACE_CONDITION_3V3_PATTERN,
            RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX,
        )?;

        // ACMD41 is an application command: CMD55 must precede every attempt.
        // The operating-conditions response (OCR) reports power-up readiness.
        let mut ocr = 0;
        for _ in 0..INITIALIZATION_ATTEMPTS {
            command(
                card_command::APPLICATION_PREFIX,
                0,
                RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX,
            )?;
            ocr = command(
                card_command::APPLICATION_OPERATING_CONDITION,
                OCR_VOLTAGE | OCR_HIGH_CAPACITY,
                RESPONSE_48,
            )?;
            if ocr & OCR_READY != 0 {
                break;
            }
        }
        if ocr & OCR_READY == 0 {
            return None;
        }

        // Request card identity, obtain its relative address, then read capacity
        // before selecting the card for data transfers.
        command(card_command::ALL_SEND_CID, 0, RESPONSE_LONG | COMMAND_CRC)?;
        let relative_address = command(
            card_command::SEND_RELATIVE_ADDRESS,
            0,
            RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX,
        )? & RELATIVE_ADDRESS_MASK;
        if relative_address == 0 {
            return None;
        }
        // CMD9 reads the Card-Specific Data register while the card is in
        // standby. Its capacity describes the whole card, not its partitions.
        command(
            card_command::SEND_CSD,
            relative_address,
            RESPONSE_LONG | COMMAND_CRC,
        )?;
        // SDHCI strips the final CRC byte from a 136-bit response. Restore
        // the bit positions used by the SD specification before extracting CSD.
        let mut csd = 0u128;
        for index in 0..4 {
            csd |= (read32(RESPONSE_0 + index * 4) as u128) << (index * 32);
        }
        csd <<= 8;
        let sector_count = sectors_from_csd(csd)?;
        if sector_count == 0 {
            return None;
        }
        command(
            card_command::SELECT_CARD,
            relative_address,
            RESPONSE_48_BUSY | COMMAND_CRC | COMMAND_INDEX,
        )?;
        wait_until(|| read32(PRESENT_STATE) & DATA_INHIBIT == 0)?;

        let high_capacity = ocr & OCR_HIGH_CAPACITY != 0;
        if !high_capacity {
            command(
                card_command::SET_BLOCK_LENGTH,
                SECTOR_BYTES as u32,
                RESPONSE_48 | COMMAND_CRC | COMMAND_INDEX,
            )?;
        }
        set_clock(source_clock_hz, TRANSFER_CLOCK_HZ)?;
        Some(Self {
            high_capacity,
            relative_address,
            sector_count,
        })
    }

    pub fn sector_count(&self) -> u64 {
        self.sector_count
    }

    pub unsafe fn try_read_sector(&mut self, sector: u64) -> Option<[u8; SECTOR_BYTES]> {
        if sector >= self.sector_count {
            return None;
        }
        let sector = u32::try_from(sector).ok()?;
        let argument = if self.high_capacity {
            sector
        } else {
            sector.checked_mul(SECTOR_BYTES as u32)?
        };
        wait_until(|| read32(PRESENT_STATE) & (COMMAND_INHIBIT | DATA_INHIBIT) == 0)?;
        write16(BLOCK_SIZE, SECTOR_BYTES as u16);
        write16(BLOCK_COUNT, 1);
        write32(INTERRUPT_STATUS, u32::MAX);
        write32(ARGUMENT, argument);
        write16(TRANSFER_MODE, TRANSFER_READ);
        write16(
            COMMAND,
            (card_command::READ_SINGLE_BLOCK << COMMAND_NUMBER_SHIFT)
                | RESPONSE_48
                | COMMAND_CRC
                | COMMAND_INDEX
                | COMMAND_DATA,
        );
        wait_interrupt(INTERRUPT_COMMAND_COMPLETE)?;
        wait_interrupt(INTERRUPT_BUFFER_READ_READY)?;

        let mut sector = [0u8; SECTOR_BYTES];
        for chunk in sector.chunks_exact_mut(DATA_WORD_BYTES) {
            chunk.copy_from_slice(&read32(BUFFER_DATA).to_le_bytes());
        }
        wait_interrupt(INTERRUPT_TRANSFER_COMPLETE)?;
        Some(sector)
    }
}

// Convert the card's encoded capacity into the 512-byte sectors used by our
// block-device API. Both formats store a size minus one.
fn sectors_from_csd(csd: u128) -> Option<u64> {
    match csd >> csd_field::FORMAT_SHIFT {
        csd_field::STANDARD_CAPACITY => {
            let block_size_exponent = ((csd >> csd_field::READ_BLOCK_EXPONENT_SHIFT)
                & csd_field::READ_BLOCK_EXPONENT_MASK) as u32;
            let size_units = ((csd >> csd_field::STANDARD_SIZE_SHIFT)
                & csd_field::STANDARD_SIZE_MASK) as u64
                + 1;
            // C_SIZE_MULT encodes a multiplier of 2^(field + 2) blocks.
            let block_count_exponent = ((csd >> csd_field::SIZE_MULTIPLIER_SHIFT)
                & csd_field::SIZE_MULTIPLIER_MASK) as u32
                + 2;
            Some(
                size_units.checked_shl(block_count_exponent + block_size_exponent)?
                    / SECTOR_BYTES as u64,
            )
        }
        csd_field::HIGH_CAPACITY => {
            let size_units = ((csd >> csd_field::HIGH_CAPACITY_SIZE_SHIFT)
                & csd_field::HIGH_CAPACITY_SIZE_MASK) as u64
                + 1;
            Some(size_units * csd_field::SECTORS_PER_CAPACITY_UNIT)
        }
        _ => None, // SDUC addressing is not implemented.
    }
}

unsafe impl Send for SdCard {}

impl BlockDevice for SdCard {
    fn read_sector(&mut self, sector: u64) -> [u8; SECTOR_BYTES] {
        unsafe { self.try_read_sector(sector).unwrap_or([0; SECTOR_BYTES]) }
    }
}

impl WritableBlockDevice for SdCard {
    fn write_sector(&mut self, sector: u64, data: &[u8; SECTOR_BYTES]) -> Result<(), BlockError> {
        if sector >= self.sector_count {
            return Err(BlockError::AddressOutOfRange);
        }
        let sector = u32::try_from(sector).map_err(|_| BlockError::AddressOutOfRange)?;
        // SDHC/SDXC cards take a sector number; standard-capacity cards take bytes.
        let argument = if self.high_capacity {
            sector
        } else {
            sector
                .checked_mul(SECTOR_BYTES as u32)
                .ok_or(BlockError::AddressOutOfRange)?
        };
        unsafe {
            wait_data_idle()?;
            write16(BLOCK_SIZE, SECTOR_BYTES as u16);
            write16(BLOCK_COUNT, 1);
            write32(INTERRUPT_STATUS, u32::MAX);
            write32(ARGUMENT, argument);
            write16(TRANSFER_MODE, TRANSFER_SINGLE_BLOCK_WRITE_PIO);
            write16(
                COMMAND,
                (card_command::WRITE_SINGLE_BLOCK << COMMAND_NUMBER_SHIFT)
                    | RESPONSE_48
                    | COMMAND_CRC
                    | COMMAND_INDEX
                    | COMMAND_DATA,
            );
            wait_interrupt_result(INTERRUPT_COMMAND_COMPLETE)?;
            check_card_status(read32(RESPONSE_0))?;

            // The controller requests one block. Each FIFO write supplies 4 bytes.
            wait_interrupt_result(INTERRUPT_BUFFER_WRITE_READY)?;
            for bytes in data.chunks_exact(DATA_WORD_BYTES) {
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
                    (card_command::SEND_STATUS << COMMAND_NUMBER_SHIFT)
                        | RESPONSE_48
                        | COMMAND_CRC
                        | COMMAND_INDEX,
                );
                wait_interrupt_result(INTERRUPT_COMMAND_COMPLETE)?;
                let status = read32(RESPONSE_0);
                check_card_status(status)?;
                let ready_for_data = status & CARD_READY_FOR_DATA != 0;
                let transfer_state =
                    (status >> CARD_STATE_SHIFT) & CARD_STATE_MASK == CARD_STATE_TRANSFER;
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
    core::ptr::read_volatile(SYSTEM_TIMER_LOW)
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

/// Poll latched status despite the register's interrupt name. A written one
/// acknowledges that status bit; no CPU interrupt handler participates here.
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
    write16(COMMAND, (index << COMMAND_NUMBER_SHIFT) | flags);
    wait_interrupt(INTERRUPT_COMMAND_COMPLETE)?;
    Some(read32(RESPONSE_0))
}

unsafe fn set_clock(source_hz: u32, maximum_hz: u32) -> Option<()> {
    // Divided mode uses source / (2 * divider). Round up so the SD clock
    // never exceeds the requested rate; encode its ten bits across CONTROL1.
    // Default-speed cards permit up to 25 MHz; initialization permits 400 kHz.
    let divider = source_hz.div_ceil(maximum_hz.checked_mul(2)?).max(1);
    if divider > CLOCK_DIVIDER_MAX {
        return None;
    }
    // Divider bits 7:0 go to register bits 15:8; bits 9:8 go to bits 7:6.
    let low_bits = (divider & CLOCK_DIVIDER_LOW_MASK) << CLOCK_DIVIDER_LOW_REGISTER_SHIFT;
    let high_bits = (divider >> CLOCK_DIVIDER_LOW_BITS) << CLOCK_DIVIDER_HIGH_REGISTER_SHIFT;
    let encoded_divider = (low_bits | high_bits) as u16;
    write16(CLOCK_CONTROL, 0);
    write16(CLOCK_CONTROL, encoded_divider | CLOCK_INTERNAL_ENABLE);
    wait_until(|| read16(CLOCK_CONTROL) & CLOCK_INTERNAL_STABLE != 0)?;
    write16(
        CLOCK_CONTROL,
        encoded_divider | CLOCK_INTERNAL_ENABLE | CLOCK_SD_ENABLE,
    );
    Some(())
}

unsafe fn wait_interrupt(mask: u32) -> Option<()> {
    wait_interrupt_result(mask).ok()
}

/// Busy-poll a register condition until it succeeds or one second elapses.
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
