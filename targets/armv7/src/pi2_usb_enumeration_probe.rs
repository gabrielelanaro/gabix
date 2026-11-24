#![no_std]
#![no_main]

#[path = "raspi2b/mailbox.rs"]
mod mailbox;
#[path = "raspi2b/usb.rs"]
mod usb;

use core::panic::PanicInfo;

core::arch::global_asm!(include_str!("pi2_usb_enumeration_probe.S"));

const USB_HUB_DEVICE_CLASS: u8 = 9;

const UART_BASE: usize = 0x3f20_1000;
const UART_FLAGS: usize = UART_BASE + 0x18;
const UART_TX_FULL: u32 = 1 << 5;

#[no_mangle]
extern "C" fn pi2_usb_enumeration_probe_main() -> ! {
    write_line(b"NET8 USB PROBE START");

    let Some(mut usb) = (unsafe { usb::Dwc2::initialize() }) else {
        write_line(b"NET8 DWC2 FAILED");
        wait_for_reset();
    };
    write_line(b"NET8 DWC2 OK");

    let Some(root) = (unsafe { usb.enumerate_root() }) else {
        write_line(b"NET8 ROOT FAILED");
        wait_for_reset();
    };
    write_device(b"NET8 ROOT ", root);

    if root.device_class == USB_HUB_DEVICE_CLASS {
        let result = unsafe {
            usb.visit_hub_children(root, |port, child| {
                write_bytes(b"NET8 PORT ");
                write_hex_u8(port);
                write_byte(b' ');
                write_device(b"DEVICE ", child);
                true
            })
        };
        if let Err(error) = result {
            write_bytes(b"NET8 HUB ERROR ");
            write_hex_u8(error as u8);
            write_byte(b'\n');
        }
    }
    // Completion also follows a reported hub error; the inventory can be partial.
    write_line(b"NET8 INVENTORY COMPLETE");
    wait_for_reset();
}

fn write_device(prefix: &[u8], device: usb::UsbDevice) {
    write_bytes(prefix);
    write_hex_u16(device.vendor_id);
    write_byte(b':');
    write_hex_u16(device.product_id);
    write_byte(b' ');
    write_hex_u8(device.device_class);
    write_byte(b'\n');
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

fn write_hex_u16(value: u16) {
    write_hex_u8((value >> 8) as u8);
    write_hex_u8(value as u8);
}

fn write_hex_u8(value: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    write_byte(HEX[(value >> 4) as usize]);
    write_byte(HEX[(value & 0x0f) as usize]);
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

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    write_line(b"NET8 PANIC");
    wait_for_reset();
}
