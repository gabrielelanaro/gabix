//! Exchange a property request in RAM with the Pi's VideoCore firmware.
//! The mailbox register carries the buffer address and channel; firmware reads
//! the request and writes its reply into that same buffer.
//! Format: raspberrypi/firmware wiki, "Mailbox property interface".

use core::arch::asm;

const BASE: usize = 0x3f00_b880;
const READ: usize = 0x00;
const STATUS: usize = 0x18;
const WRITE: usize = 0x20;

const STATUS_FULL: u32 = 1 << 31;
const STATUS_EMPTY: u32 = 1 << 30;
const PROPERTY_CHANNEL: u32 = 8;
const RESPONSE_SUCCESS: u32 = 0x8000_0000;
const SET_POWER_STATE: u32 = 0x0002_8001;
const USB_HCD_DEVICE: u32 = 3;
const POWER_ON: u32 = 1 << 0;
const WAIT_FOR_STABLE_POWER: u32 = 1 << 1;
const PROCESS_REQUEST: u32 = 0;
const TAG_REQUEST: u32 = 0;
const END_TAG: u32 = 0;
const MESSAGE_BYTES: u32 = 32;
const VALUE_BYTES: u32 = 8; // Two u32 values: resource ID and state/rate.
const BUFFER_RESPONSE_WORD: usize = 1;
const TAG_RESPONSE_WORD: usize = 4;
const RESOURCE_ID_WORD: usize = 5;
const VALUE_WORD: usize = 6;
const RAM_ADDRESS_MASK: u32 = 0x3fff_ffff;
const VIDEOCORE_BUS_ALIAS: u32 = 0xc000_0000;
// Iteration bound, not a duration: each attempt polls a device register.
const POLL_LIMIT: usize = 10_000_000;

// Firmware requires 16-byte alignment; 64 also isolates our cache maintenance
// from adjacent objects. Only the first 32 bytes belong to the wire message.
#[repr(C, align(64))]
struct PropertyMessage([u32; 8]);

static mut MESSAGE: PropertyMessage = PropertyMessage([0; 8]);

/// Caller must serialize mailbox requests: both operations share MESSAGE.
pub unsafe fn power_on_usb() -> Option<()> {
    let returned_state = exchange_id_value(
        SET_POWER_STATE,
        USB_HCD_DEVICE,
        POWER_ON | WAIT_FOR_STABLE_POWER,
    )?;
    (returned_state & POWER_ON != 0).then_some(())
}

/// Firmware owns the EMMC source clock. Query it before calculating SD dividers.
/// Caller must serialize requests with power_on_usb: both share MESSAGE.
pub unsafe fn emmc_clock_rate() -> Option<u32> {
    const GET_CLOCK_RATE: u32 = 0x0003_0002;
    const EMMC_CLOCK: u32 = 1;
    // This query supplies only the clock ID; its output value starts empty.
    let rate = exchange_id_value(GET_CLOCK_RATE, EMMC_CLOCK, 0)?;
    (rate != 0).then_some(rate)
}

// Both supported tags exchange an ID and one value. Keep the wire offsets
// visible here so the response checks can be read against the request layout.
unsafe fn exchange_id_value(tag: u32, id: u32, value: u32) -> Option<u32> {
    let message = &mut *core::ptr::addr_of_mut!(MESSAGE);
    message.0 = [
        MESSAGE_BYTES,
        PROCESS_REQUEST,
        tag,
        VALUE_BYTES,
        TAG_REQUEST,
        id,
        value,
        END_TAG,
    ];
    let address = bus_address(core::ptr::addr_of!(MESSAGE) as u32);
    // Alignment leaves the low four address bits free for the channel number.
    let request = address | PROPERTY_CHANNEL;

    // Publish the RAM request before notifying firmware through the mailbox.
    clean_invalidate_message();
    wait_until(|| read32(STATUS) & STATUS_FULL == 0)?;
    write32(WRITE, request);
    for _ in 0..POLL_LIMIT {
        if read32(STATUS) & STATUS_EMPTY != 0 {
            core::hint::spin_loop();
            continue;
        }
        // Accept the reply identifying this buffer and property channel.
        // Reads remove mailbox entries; unmatched replies are discarded.
        if read32(READ) == request {
            // Discard the CPU's cached request before reading firmware's reply.
            invalidate_message();
            return (message.0[BUFFER_RESPONSE_WORD] == RESPONSE_SUCCESS
                && message.0[TAG_RESPONSE_WORD] == (RESPONSE_SUCCESS | VALUE_BYTES)
                && message.0[RESOURCE_ID_WORD] == id)
                .then_some(message.0[VALUE_WORD]);
        }
    }
    None
}

unsafe fn clean_invalidate_message() {
    let address = core::ptr::addr_of!(MESSAGE) as u32;
    asm!("mcr p15, 0, {address}, c7, c14, 1", address = in(reg) address);
    asm!("dsb sy");
}

unsafe fn invalidate_message() {
    let address = core::ptr::addr_of!(MESSAGE) as u32;
    asm!("mcr p15, 0, {address}, c7, c6, 1", address = in(reg) address);
    asm!("dsb sy");
}

const fn bus_address(address: u32) -> u32 {
    (address & RAM_ADDRESS_MASK) | VIDEOCORE_BUS_ALIAS
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

unsafe fn read32(offset: usize) -> u32 {
    core::ptr::read_volatile((BASE + offset) as *const u32)
}

unsafe fn write32(offset: usize, value: u32) {
    core::ptr::write_volatile((BASE + offset) as *mut u32, value);
}
