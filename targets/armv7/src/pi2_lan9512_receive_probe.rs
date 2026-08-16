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
#[cfg(feature = "pi-network-arp-reply-probe")]
use gabix_kernel::net::stack::{NetworkInterface, MAX_FRAME_LEN};

core::arch::global_asm!(include_str!("pi2_lan9512_receive_probe.S"));

const UART_BASE: usize = 0x3f20_1000;
const UART_FLAGS: usize = UART_BASE + 0x18;
const UART_TX_FULL: u32 = 1 << 5;
const SYSTEM_TIMER_LOW: *const u32 = 0x3f00_3004 as *const u32;

#[no_mangle]
extern "C" fn pi2_lan9512_receive_probe_main() -> ! {
    write_line(b"NET11 LAN9512 RX START");
    if !unsafe { memory::enable_smp() } {
        fail(b"NET11 SMP FAILED");
    }
    if !unsafe { memory::enable_identity_mapping() } {
        fail(b"NET11 MMU FAILED");
    }
    unsafe { allocator::init() };
    write_line(b"NET11 BASELINE OK");

    let Some(mut usb) = (unsafe { usb::Dwc2::initialize() }) else {
        fail(b"NET11 DWC2 FAILED");
    };
    let Some(root) = (unsafe { usb.enumerate_root() }) else {
        fail(b"NET11 ROOT FAILED");
    };
    let network_device = if root.device_class == 9 {
        let child = match unsafe { usb.enumerate_hub_child_diagnostic(root) } {
            Ok(child) => child,
            Err(error) => {
                write_bytes(b"NET11 CHILD FAILED ");
                write_hex_u8(error as u8);
                write_byte(b'\n');
                fail(b"NET11 CHILD DETAIL");
            }
        };
        child
    } else {
        root
    };

    let Some(mut network) = (unsafe { lan9512::Lan9512Ethernet::configure(usb, network_device) })
    else {
        fail(b"NET11 LAN9512 CONFIG FAILED");
    };
    let mac = network.mac_address();
    write_bytes(b"NET11 LAN9512 MAC ");
    write_mac(mac);
    write_byte(b'\n');
    write_line(b"NET11 LAN9512 CONFIG OK");
    write_line(b"NET11 WAITING FOR LINK");
    delay_us(5_000_000);

    write_line(b"NET11 RX POLL");
    let started = timer_now();
    #[cfg(feature = "pi-network-repeated-receive-probe")]
    let mut received = 0usize;
    while timer_now().wrapping_sub(started) < 5_000_000 {
        if let Some(frame) = network.receive() {
            let bytes = frame.bytes();
            #[cfg(feature = "pi-network-repeated-receive-probe")]
            {
                received += 1;
                if received < 16 {
                    continue;
                }
            }
            #[cfg(feature = "pi-network-arp-receive-probe")]
            if bytes.get(12..14) != Some(&[0x08, 0x06]) {
                continue;
            }
            write_bytes(b"NET11 RX LEN ");
            write_hex_u16(bytes.len() as u16);
            if bytes.len() >= 14 {
                write_bytes(b" DST ");
                write_mac_slice(&bytes[..6]);
                write_bytes(b" SRC ");
                write_mac_slice(&bytes[6..12]);
                write_bytes(b" TYPE ");
                write_hex_u8(bytes[12]);
                write_hex_u8(bytes[13]);
            }
            write_byte(b'\n');
            #[cfg(feature = "pi-network-arp-reply-probe")]
            {
                let interface = NetworkInterface::new(mac, [10, 0, 2, 15]);
                let mut reply = [0u8; MAX_FRAME_LEN];
                let Some(length) = interface.reply(bytes, &mut reply) else {
                    fail(b"NET11 ARP REPLY BUILD FAILED");
                };
                if unsafe { network.transmit_frame(&reply[..length]) } {
                    write_line(b"NET11 ARP REPLY TX OK");
                } else {
                    fail(b"NET11 ARP REPLY TX FAILED");
                }
            }
            wait_for_reset();
        }
    }
    #[cfg(feature = "pi-network-repeated-receive-probe")]
    {
        write_bytes(b"NET11 RX COUNT ");
        write_hex_u16(received as u16);
        write_byte(b'\n');
    }
    write_line(b"NET11 RX NONE");
    wait_for_reset();
}

fn write_mac(mac: [u8; 6]) {
    write_mac_slice(&mac);
}

fn write_mac_slice(mac: &[u8]) {
    for (index, byte) in mac.iter().enumerate() {
        if index != 0 {
            write_byte(b':');
        }
        write_hex_u8(*byte);
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

fn delay_us(microseconds: u32) {
    let start = timer_now();
    while timer_now().wrapping_sub(start) < microseconds {
        core::hint::spin_loop();
    }
}

fn timer_now() -> u32 {
    unsafe { core::ptr::read_volatile(SYSTEM_TIMER_LOW) }
}

fn wait_for_reset() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[alloc_error_handler]
fn allocation_error(_: Layout) -> ! {
    fail(b"NET11 ALLOC FAILED");
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    fail(b"NET11 PANIC");
}
