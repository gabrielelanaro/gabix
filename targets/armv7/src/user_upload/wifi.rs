//! One authenticated ELF upload in RAM. Network handling queues entry only;
//! the foreground service enters user mode after the signed reply is cached.
use crate::{memory, user};
use core::ptr::addr_of_mut;
use gabix_kernel::{boot_record::crc32, elf::Elf32};

const BEGIN: u32 = 24;
const WRITE: u32 = 25;
const START: u32 = 26;
pub const RESULT: u32 = 27;
const INVALID: u32 = 1;
const ORDER: u32 = 2;
const CHECKSUM: u32 = 4;
const MAX_IMAGE: usize = super::MAX_USER_ELF_SIZE as usize;

enum State {
    Idle,
    Receiving,
    Ready(u32),
    Running,
    Exited(u32),
}

struct Upload {
    session: u32,
    size: usize,
    received: usize,
    checksum: u32,
    state: State,
}

// One foreground network receiver. The exit handler accesses this state only
// after the foreground borrow has ended and control has passed to user mode.
static mut IMAGE: [u8; MAX_IMAGE] = [0; MAX_IMAGE];
static mut UPLOAD: Upload = Upload {
    session: 0,
    size: 0,
    received: 0,
    checksum: 0,
    state: State::Idle,
};

pub fn command(
    op: u32,
    session: u32,
    arg: u32,
    value: u32,
    data: &[u8],
    reply: &mut [u8],
) -> Option<Result<usize, u32>> {
    if !(BEGIN..=RESULT).contains(&op) {
        return None;
    }
    let upload = unsafe { &mut *addr_of_mut!(UPLOAD) };
    let image = unsafe { &mut *addr_of_mut!(IMAGE) };
    let offset = arg as usize;
    Some(match op {
        BEGIN if session != 0 && (1..=MAX_IMAGE).contains(&offset) && data.is_empty() => {
            *upload = Upload {
                session,
                size: offset,
                received: 0,
                checksum: value,
                state: State::Receiving,
            };
            Ok(0)
        }
        _ if session == 0 || session != upload.session => Err(ORDER),
        WRITE
            if matches!(upload.state, State::Receiving)
                && offset == upload.received
                && (1..=512).contains(&data.len())
                && data.len() <= upload.size - offset =>
        {
            image[offset..offset + data.len()].copy_from_slice(data);
            upload.received += data.len();
            Ok(0)
        }
        START
            if matches!(upload.state, State::Receiving)
                && upload.received == upload.size
                && data.is_empty() =>
        {
            let bytes = &image[..upload.size];
            if crc32(bytes) != upload.checksum {
                Err(CHECKSUM)
            } else if let Some(entry) = load(bytes) {
                upload.state = State::Ready(entry);
                Ok(0)
            } else {
                Err(INVALID)
            }
        }
        RESULT if data.is_empty() => match upload.state {
            State::Exited(code) => {
                reply[..4].copy_from_slice(&code.to_le_bytes());
                Ok(4)
            }
            State::Ready(_) | State::Running => Ok(0),
            _ => Err(ORDER),
        },
        _ => Err(INVALID),
    })
}

fn load(bytes: &[u8]) -> Option<u32> {
    let elf = Elf32::parse(bytes)?;
    let entry = elf.entry();
    let mut executable_entry = false;
    // Validate every segment before copying any. The bootstrap ABI runs ARM
    // instructions and accepts only the user image range supported by write().
    let valid = elf.load_segments(|s| {
        let Some(end) = s.virtual_address.checked_add(s.memory_size) else {
            return false;
        };
        if s.virtual_address < memory::PROCESS_BASE || end > memory::PROCESS_END {
            return false;
        }
        if s.flags & 1 != 0 && entry >= s.virtual_address {
            executable_entry |= (entry - s.virtual_address) as usize + 4 <= s.data.len();
        }
        true
    });
    if !valid || !executable_entry || entry % 4 != 0 {
        return None;
    }
    unsafe { user::load_elf(bytes) }
}

pub fn run_pending() {
    let upload = unsafe { &mut *addr_of_mut!(UPLOAD) };
    if let State::Ready(entry) = upload.state {
        upload.state = State::Running;
        unsafe {
            memory::allow_user_section(user::USER_STACK_TOP - 1);
            // The user alias is non-cacheable. Finish the copy, then discard
            // instructions and branch predictions left by the previous image.
            core::arch::asm!("dsb sy", "mcr p15, 0, {z}, c7, c5, 0",
                "mcr p15, 0, {z}, c7, c5, 6", "dsb sy", "isb", z = in(reg) 0u32);
            crate::raspi2b::reset::arm_deadman(super::USER_EXECUTION_DEADMAN_SECONDS);
            user::enter(entry, user::USER_STACK_TOP);
        }
    }
}

pub fn running() -> bool {
    unsafe { matches!((*core::ptr::addr_of!(UPLOAD)).state, State::Running) }
}

pub fn exited(code: u32) {
    super::tcp::reset();
    let upload = unsafe { &mut *addr_of_mut!(UPLOAD) };
    if matches!(upload.state, State::Running) {
        upload.state = State::Exited(code);
    }
}
