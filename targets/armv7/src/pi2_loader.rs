#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
use gabix_kernel::loader_protocol::{UploadEvent, UploadReceiver};

core::arch::global_asm!(include_str!("pi2_loader.S"));

const UART_BASE: usize = 0x3f20_1000;
const UART_DATA: usize = 0x00;
const UART_FLAGS: usize = 0x18;
const UART_RX_EMPTY: u32 = 1 << 4;
const UART_TX_FULL: u32 = 1 << 5;
const UART_BUSY: u32 = 1 << 3;

const SYSTEM_TIMER_LOW: usize = 0x3f00_3004;
const UPLOAD_TIMEOUT_MICROSECONDS: u32 = 5_000_000;
const DEVELOPMENT_KERNEL_ADDRESS: usize = 0x0010_0000;

#[no_mangle]
extern "C" fn pi2_loader_main() -> ! {
    let uart = Uart;
    let mut receiver = UploadReceiver::new();
    uart.write(b"GABIX LOADER READY\r\n");

    loop {
        let byte = if receiver.is_receiving_payload() {
            match uart.read_with_timeout(UPLOAD_TIMEOUT_MICROSECONDS) {
                Some(byte) => byte,
                None => {
                    handle_event(&uart, receiver.timeout());
                    continue;
                }
            }
        } else {
            uart.read_blocking()
        };

        handle_event(&uart, receiver.push(byte));
    }
}

fn handle_event(uart: &Uart, event: UploadEvent) {
    match event {
        UploadEvent::Waiting => {}
        UploadEvent::Payload { offset, byte } => write_kernel_byte(offset, byte),
        UploadEvent::Complete { offset, byte, .. } => {
            write_kernel_byte(offset, byte);
            uart.write(b"UPLOAD OK\r\n");
            uart.flush();
            start_development_kernel();
        }
        UploadEvent::Failed => {
            uart.write(b"UPLOAD FAILED\r\n");
            uart.write(b"GABIX LOADER READY\r\n");
        }
    }
}

fn write_kernel_byte(offset: u32, byte: u8) {
    unsafe {
        core::ptr::write_volatile(
            (DEVELOPMENT_KERNEL_ADDRESS + offset as usize) as *mut u8,
            byte,
        );
    }
}

fn start_development_kernel() -> ! {
    unsafe {
        asm!(
            "dsb sy",
            "mov r0, #0",
            "mcr p15, 0, r0, c7, c5, 0",
            "isb",
            "bx {entry}",
            entry = in(reg) DEVELOPMENT_KERNEL_ADDRESS,
            options(noreturn)
        );
    }
}

struct Uart;

impl Uart {
    fn read_blocking(&self) -> u8 {
        loop {
            if let Some(byte) = self.try_read() {
                return byte;
            }
            core::hint::spin_loop();
        }
    }

    fn read_with_timeout(&self, timeout_microseconds: u32) -> Option<u8> {
        let started = timer_low();
        loop {
            if let Some(byte) = self.try_read() {
                return Some(byte);
            }
            if timer_low().wrapping_sub(started) >= timeout_microseconds {
                return None;
            }
            core::hint::spin_loop();
        }
    }

    fn try_read(&self) -> Option<u8> {
        unsafe {
            if read32(UART_BASE + UART_FLAGS) & UART_RX_EMPTY != 0 {
                None
            } else {
                Some(read32(UART_BASE + UART_DATA) as u8)
            }
        }
    }

    fn write(&self, bytes: &[u8]) {
        for &byte in bytes {
            unsafe {
                while read32(UART_BASE + UART_FLAGS) & UART_TX_FULL != 0 {
                    core::hint::spin_loop();
                }
                write32(UART_BASE + UART_DATA, byte as u32);
            }
        }
    }

    fn flush(&self) {
        unsafe {
            while read32(UART_BASE + UART_FLAGS) & UART_BUSY != 0 {
                core::hint::spin_loop();
            }
        }
    }
}

fn timer_low() -> u32 {
    unsafe { read32(SYSTEM_TIMER_LOW) }
}

unsafe fn read32(address: usize) -> u32 {
    core::ptr::read_volatile(address as *const u32)
}

unsafe fn write32(address: usize, value: u32) {
    core::ptr::write_volatile(address as *mut u32, value);
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
