#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
use gabix_kernel::loader_protocol::{UploadEvent, UploadFailure, UploadReceiver};

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
            uart.write(b"UPLOAD CRC OK\r\n");
            uart.write(b"UPLOAD OK\r\n");
            uart.write(b"HANDOFF ADDRESS ");
            uart.write_hex32(DEVELOPMENT_KERNEL_ADDRESS as u32);
            uart.write(b"\r\nHANDOFF LENGTH ");
            uart.write_hex32(offset + 1);
            uart.write(b"\r\nHANDOFF FIRST WORD ");
            uart.write_hex32(unsafe { read32(DEVELOPMENT_KERNEL_ADDRESS) });
            uart.write(b"\r\nHANDOFF SCTLR ");
            uart.write_hex32(read_sctlr());
            uart.write(b"\r\nHANDOFF CPSR ");
            uart.write_hex32(read_cpsr());
            uart.write(b"\r\nHANDOFF BRANCH\r\n");
            uart.flush();
            call_development_kernel();
            uart.write(b"\r\nHANDOFF RETURNED\r\n");
            uart.write(b"GABIX LOADER READY\r\n");
            uart.flush();
        }
        UploadEvent::Failed { reason } => {
            match reason {
                UploadFailure::InvalidHeader => uart.write(b"UPLOAD HEADER FAILED\r\n"),
                UploadFailure::CrcMismatch => uart.write(b"UPLOAD CRC FAILED\r\n"),
                UploadFailure::Timeout => uart.write(b"UPLOAD TIMEOUT\r\n"),
            }
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

fn call_development_kernel() {
    unsafe {
        asm!(
            "blx {entry}",
            entry = in(reg) DEVELOPMENT_KERNEL_ADDRESS,
            clobber_abi("C"),
        );
    }
}

fn read_sctlr() -> u32 {
    let value: u32;
    unsafe {
        asm!(
            "mrc p15, 0, {value}, c1, c0, 0",
            value = out(reg) value,
            options(nostack, preserves_flags)
        );
    }
    value
}

fn read_cpsr() -> u32 {
    let value: u32;
    unsafe {
        asm!(
            "mrs {value}, cpsr",
            value = out(reg) value,
            options(nostack, preserves_flags)
        );
    }
    value
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

    fn write_hex32(&self, value: u32) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        self.write(b"0x");
        for shift in (0..8).rev() {
            self.write(&[HEX[((value >> (shift * 4)) & 0xf) as usize]]);
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
