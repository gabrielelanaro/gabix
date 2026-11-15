use crate::console::{write_line, Pl011};
use crate::{memory, user};
use gabix_kernel::loader_protocol::{UploadEvent, UploadFailure, UploadReceiver};

const MAX_USER_ELF_SIZE: u32 = 512 * 1024;
const UPLOAD_IDLE_TIMEOUT_TICKS: u64 = 100;
const USER_EXECUTION_DEADMAN_SECONDS: u32 = 10;
const ETHERNET_POLL_INTERVAL_TICKS: u64 = 1;

#[repr(C, align(16))]
struct UserElfBuffer([u8; MAX_USER_ELF_SIZE as usize]);

static mut USER_ELF: UserElfBuffer = UserElfBuffer([0; MAX_USER_ELF_SIZE as usize]);

pub fn serve_uart_uploads_and_ethernet(uart_console: &mut Pl011) -> ! {
    let mut uart_upload_receiver = UploadReceiver::with_max_size(MAX_USER_ELF_SIZE);
    let mut last_uart_byte_tick = crate::raspi2b::timer::ticks();
    let mut next_ethernet_poll_tick = last_uart_byte_tick;
    let mut next_dhcp_discover_tick = last_uart_byte_tick;
    #[cfg(feature = "pi-sd-transfer")]
    crate::confirm_sd_boot(uart_console);
    write_line(uart_console, b"GABIX USER READY");
    #[cfg(feature = "pi-sd-transfer")]
    crate::flush_diagnostics(true);

    loop {
        #[cfg(feature = "pi-sd-transfer")]
        crate::flush_diagnostics(false);
        // 1. Read one UART byte, or detect an interrupted upload.
        let uart_upload_event = if let Some(byte) = uart_console.try_read_byte() {
            last_uart_byte_tick = crate::raspi2b::timer::ticks();
            Some(uart_upload_receiver.push(byte))
        } else if uart_upload_receiver.is_upload_active()
            && crate::raspi2b::timer::ticks().wrapping_sub(last_uart_byte_tick)
                >= UPLOAD_IDLE_TIMEOUT_TICKS
        {
            Some(uart_upload_receiver.timeout())
        } else {
            None
        };
        let Some(uart_upload_event) = uart_upload_event else {
            let current_tick = crate::raspi2b::timer::ticks();
            // 2. With no UART event, give Ethernet a turn when its poll is due.
            // Pause Ethernet during uploads so the UART FIFO can be drained promptly.
            let uart_upload_in_progress = uart_upload_receiver.is_upload_active();
            let ethernet_poll_is_due = current_tick >= next_ethernet_poll_tick;
            if !uart_upload_in_progress && ethernet_poll_is_due {
                crate::poll_ethernet_and_reply(uart_console, &mut next_dhcp_discover_tick);
                next_ethernet_poll_tick = current_tick + ETHERNET_POLL_INTERVAL_TICKS;
            }
            core::hint::spin_loop();
            continue;
        };
        // 3. Handle the UART upload event before checking either device again.
        match uart_upload_event {
            UploadEvent::Waiting => {}
            UploadEvent::Payload { offset, byte } => write_byte(offset, byte),
            UploadEvent::Complete {
                offset, byte, size, ..
            } => {
                write_byte(offset, byte);
                write_line(uart_console, b"USER UPLOAD CRC OK");
                let image = unsafe {
                    core::slice::from_raw_parts(
                        core::ptr::addr_of!(USER_ELF).cast::<u8>(),
                        size as usize,
                    )
                };
                if let Some(entry) = unsafe { user::load_elf(image) } {
                    write_line(uart_console, b"USER UPLOAD OK");
                    unsafe {
                        memory::allow_user_section(user::USER_STACK_TOP - 1);
                        #[cfg(feature = "pi-hardware")]
                        crate::raspi2b::reset::arm_deadman(USER_EXECUTION_DEADMAN_SECONDS);
                        user::enter(entry, user::USER_STACK_TOP);
                    }
                }
                write_line(uart_console, b"USER ELF FAILED");
                write_line(uart_console, b"USER UPLOAD FAILED");
                write_line(uart_console, b"GABIX USER READY");
            }
            UploadEvent::Failed { reason } => {
                match reason {
                    UploadFailure::InvalidHeader => {
                        write_line(uart_console, b"USER UPLOAD HEADER FAILED")
                    }
                    UploadFailure::CrcMismatch => {
                        write_line(uart_console, b"USER UPLOAD CRC FAILED")
                    }
                    UploadFailure::Timeout => write_line(uart_console, b"USER UPLOAD TIMEOUT"),
                }
                write_line(uart_console, b"USER UPLOAD FAILED");
                write_line(uart_console, b"GABIX USER READY");
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
