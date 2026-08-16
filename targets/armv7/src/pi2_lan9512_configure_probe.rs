#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod allocator;
#[path = "raspi2b/lan9512.rs"]
mod lan9512;
#[path = "raspi2b/mailbox.rs"]
mod mailbox;
mod memory;
#[path = "raspi2b/usb.rs"]
mod usb;

use core::alloc::Layout;
use core::panic::PanicInfo;
use gabix_kernel::device::FrameDevice;

core::arch::global_asm!(include_str!("pi2_lan9512_configure_probe.S"));

const UART_BASE: usize = 0x3f20_1000;
const UART_FLAGS: usize = UART_BASE + 0x18;
const UART_TX_FULL: u32 = 1 << 5;

#[no_mangle]
extern "C" fn pi2_lan9512_configure_probe_main() -> ! {
    write_line(b"NET9 LAN9512 CONFIG START");
    if !unsafe { memory::enable_smp() } {
        fail(b"NET9 SMP FAILED");
    }
    if !unsafe { memory::enable_identity_mapping() } {
        fail(b"NET9 MMU FAILED");
    }
    unsafe { allocator::init() };
    write_line(b"NET9 BASELINE OK");

    let Some(mut usb) = (unsafe { usb::Dwc2::initialize() }) else {
        fail(b"NET9 DWC2 FAILED");
    };
    let Some(root) = (unsafe { usb.enumerate_root() }) else {
        fail(b"NET9 ROOT FAILED");
    };
    let network_device = if root.device_class == 9 {
        let child = match unsafe { usb.enumerate_hub_child_diagnostic(root) } {
            Ok(child) => child,
            Err(error) => {
                write_bytes(b"NET9 CHILD FAILED ");
                write_hex_u8(error as u8);
                write_byte(b'\n');
                fail(b"NET9 CHILD DETAIL");
            }
        };
        child
    } else {
        root
    };

    let Some(network) = (unsafe { lan9512::Lan9512Ethernet::configure(usb, network_device) })
    else {
        fail(b"NET9 LAN9512 CONFIG FAILED");
    };
    write_bytes(b"NET9 LAN9512 MAC ");
    write_mac(network.mac_address());
    write_byte(b'\n');
    write_line(b"NET9 LAN9512 CONFIG OK");
    wait_for_reset();
}

fn write_mac(mac: [u8; 6]) {
    for (index, byte) in mac.iter().enumerate() {
        if index != 0 {
            write_byte(b':');
        }
        write_hex_u8(*byte);
    }
}

fn write_hex_u8(value: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    write_byte(HEX[(value >> 4) as usize]);
    write_byte(HEX[(value & 0x0f) as usize]);
}

fn fail(message: &[u8]) -> ! {
    write_line(message);
    wait_for_reset();
}

fn write_line(bytes: &[u8]) {
    write_bytes(bytes);
    write_byte(b'\n');
}

fn write_bytes(bytes: &[u8]) {
    for byte in bytes {
        write_byte(*byte);
    }
}

fn write_byte(byte: u8) {
    unsafe {
        while core::ptr::read_volatile(UART_FLAGS as *const u32) & UART_TX_FULL != 0 {}
        core::ptr::write_volatile(UART_BASE as *mut u32, byte as u32);
    }
}

fn wait_for_reset() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[alloc_error_handler]
fn allocation_error(_: Layout) -> ! {
    fail(b"NET9 ALLOC FAILED");
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    fail(b"NET9 PANIC");
}
