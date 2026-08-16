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
const POWER_ON_WAIT: u32 = 3;
const POLL_LIMIT: usize = 10_000_000;

#[repr(C, align(64))]
struct PropertyMessage([u32; 8]);

static mut MESSAGE: PropertyMessage = PropertyMessage([0; 8]);

pub unsafe fn power_on_usb() -> Option<()> {
    let message = &mut *core::ptr::addr_of_mut!(MESSAGE);
    message.0 = [
        32,
        0,
        SET_POWER_STATE,
        8,
        8,
        USB_HCD_DEVICE,
        POWER_ON_WAIT,
        0,
    ];
    let address = bus_address(core::ptr::addr_of!(MESSAGE) as u32);
    let request = address | PROPERTY_CHANNEL;

    clean_invalidate_message();
    wait_until(|| read32(STATUS) & STATUS_FULL == 0)?;
    write32(WRITE, request);
    for _ in 0..POLL_LIMIT {
        if read32(STATUS) & STATUS_EMPTY != 0 {
            core::hint::spin_loop();
            continue;
        }
        if read32(READ) == request {
            invalidate_message();
            return (message.0[1] == RESPONSE_SUCCESS
                && message.0[4] & RESPONSE_SUCCESS != 0
                && message.0[6] & 1 != 0)
                .then_some(());
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
    (address & 0x3fff_ffff) | 0xc000_0000
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
