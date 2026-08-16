use crate::console::{write_line, Pl011};
use crate::{memory, user};
use gabix_kernel::loader_protocol::{UploadEvent, UploadFailure, UploadReceiver};

const MAX_USER_ELF_SIZE: u32 = 512 * 1024;
const UPLOAD_IDLE_TIMEOUT_TICKS: u64 = 100;
const USER_EXECUTION_DEADMAN_SECONDS: u32 = 10;

#[repr(C, align(16))]
struct UserElfBuffer([u8; MAX_USER_ELF_SIZE as usize]);

static mut USER_ELF: UserElfBuffer = UserElfBuffer([0; MAX_USER_ELF_SIZE as usize]);

pub fn run(console: &mut Pl011) -> ! {
    let mut receiver = UploadReceiver::with_max_size(MAX_USER_ELF_SIZE);
    let mut last_activity = crate::raspi2b::timer::ticks();
    write_line(console, b"GABIX USER READY");

    loop {
        let event = if let Some(byte) = console.try_read_byte() {
            last_activity = crate::raspi2b::timer::ticks();
            Some(receiver.push(byte))
        } else if receiver.is_upload_active()
            && crate::raspi2b::timer::ticks().wrapping_sub(last_activity)
                >= UPLOAD_IDLE_TIMEOUT_TICKS
        {
            Some(receiver.timeout())
        } else {
            None
        };
        let Some(event) = event else {
            core::hint::spin_loop();
            continue;
        };
        match event {
            UploadEvent::Waiting => {}
            UploadEvent::Payload { offset, byte } => write_byte(offset, byte),
            UploadEvent::Complete {
                offset, byte, size, ..
            } => {
                write_byte(offset, byte);
                write_line(console, b"USER UPLOAD CRC OK");
                let image = unsafe {
                    core::slice::from_raw_parts(
                        core::ptr::addr_of!(USER_ELF).cast::<u8>(),
                        size as usize,
                    )
                };
                if let Some(entry) = unsafe { user::load_elf(image) } {
                    write_line(console, b"USER UPLOAD OK");
                    unsafe {
                        memory::allow_user_section(user::USER_STACK_TOP - 1);
                        #[cfg(feature = "pi-hardware")]
                        crate::raspi2b::reset::arm_deadman(USER_EXECUTION_DEADMAN_SECONDS);
                        user::enter(entry, user::USER_STACK_TOP);
                    }
                }
                write_line(console, b"USER ELF FAILED");
                write_line(console, b"USER UPLOAD FAILED");
                write_line(console, b"GABIX USER READY");
            }
            UploadEvent::Failed { reason } => {
                match reason {
                    UploadFailure::InvalidHeader => {
                        write_line(console, b"USER UPLOAD HEADER FAILED")
                    }
                    UploadFailure::CrcMismatch => write_line(console, b"USER UPLOAD CRC FAILED"),
                    UploadFailure::Timeout => write_line(console, b"USER UPLOAD TIMEOUT"),
                }
                write_line(console, b"USER UPLOAD FAILED");
                write_line(console, b"GABIX USER READY");
            }
        }
    }
}

fn write_byte(offset: u32, byte: u8) {
    unsafe {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(USER_ELF)
                .cast::<u8>()
                .add(offset as usize),
            byte,
        );
    }
}
