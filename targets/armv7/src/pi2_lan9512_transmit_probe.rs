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
#[cfg(feature = "pi-network-dhcp-packet-probe")]
#[path = "network.rs"]
mod network_protocol;
#[path = "raspi2b/usb.rs"]
mod usb;

use core::alloc::Layout;
use core::panic::PanicInfo;
use gabix_kernel::device::FrameDevice;

core::arch::global_asm!(include_str!("pi2_lan9512_transmit_probe.S"));

const USB_HUB_DEVICE_CLASS: u8 = 9;

const UART_BASE: usize = 0x3f20_1000;
const UART_FLAGS: usize = UART_BASE + 0x18;
const UART_TX_FULL: u32 = 1 << 5;
const LINK_SETTLE_DELAY_MICROS: u32 = 5_000_000;
const PROBE_FRAME_COUNT: u8 = 16;
const SYSTEM_TIMER_LOW: *const u32 = 0x3f00_3004 as *const u32;
#[cfg(feature = "pi-network-dhcp-size-probe")]
const FRAME_LEN: usize = 293; // Historical DHCP-size comparison payload.
#[cfg(not(feature = "pi-network-dhcp-size-probe"))]
const FRAME_LEN: usize = 60; // Ethernet minimum excluding the hardware-added FCS.
const PROBE_ETHERTYPE: [u8; 2] = [0x88, 0xb5];
#[cfg(feature = "pi-network-dhcp-packet-probe")]
const PROBE_DHCP_TRANSACTION_ID: u32 = 0x4741_4258;

#[no_mangle]
extern "C" fn pi2_lan9512_transmit_probe_main() -> ! {
    write_line(b"NET10 LAN9512 TX START");
    if !unsafe { memory::enable_smp() } {
        fail(b"NET10 SMP FAILED");
    }
    if !unsafe { memory::enable_identity_mapping() } {
        fail(b"NET10 MMU FAILED");
    }
    unsafe { allocator::init() };
    write_line(b"NET10 BASELINE OK");

    let Some(mut usb) = (unsafe { usb::Dwc2::initialize() }) else {
        fail(b"NET10 DWC2 FAILED");
    };
    let Some(root) = (unsafe { usb.enumerate_root() }) else {
        fail(b"NET10 ROOT FAILED");
    };
    // This diagnostic selects the first enumerated child, without searching by device ID.
    let network_device = if root.device_class == USB_HUB_DEVICE_CLASS {
        let child = match unsafe { usb.enumerate_hub_child_diagnostic(root) } {
            Ok(child) => child,
            Err(error) => {
                write_bytes(b"NET10 CHILD FAILED ");
                write_hex_u8(error as u8);
                write_byte(b'\n');
                fail(b"NET10 CHILD DETAIL");
            }
        };
        child
    } else {
        root
    };

    let Some(mut network) = (unsafe { lan9512::Lan9512Ethernet::configure(usb, network_device) })
    else {
        fail(b"NET10 LAN9512 CONFIG FAILED");
    };
    let mac = network.mac_address();
    write_bytes(b"NET10 LAN9512 MAC ");
    write_mac(mac);
    write_byte(b'\n');
    write_line(b"NET10 LAN9512 CONFIG OK");
    write_line(b"NET10 WAITING FOR LINK");
    // Fixed settling delay; this probe does not read PHY link status here.
    delay_us(LINK_SETTLE_DELAY_MICROS);

    let mut transmitted = 0;

    #[cfg(feature = "pi-network-dhcp-packet-probe")]
    {
        let session = network_protocol::DhcpSession::new(mac, PROBE_DHCP_TRANSACTION_ID);
        for _ in 0..PROBE_FRAME_COUNT {
            if session.send_discover(&mut network) {
                transmitted += 1;
            }
        }
    }

    #[cfg(not(feature = "pi-network-dhcp-packet-probe"))]
    {
        let mut frame = [0u8; FRAME_LEN];
        frame[..6].fill(0xff);
        frame[6..12].copy_from_slice(&mac);
        frame[12..14].copy_from_slice(&PROBE_ETHERTYPE);
        frame[14..31].copy_from_slice(b"GABIX-NET10-PROBE");
        for sequence in 0..PROBE_FRAME_COUNT {
            frame[31] = sequence;
            if unsafe { network.transmit_frame(&frame) } {
                transmitted += 1;
            }
        }
    }
    // Counts successful USB bulk submissions; peer receipt requires a host capture.
    write_bytes(b"NET10 TX SUCCEEDED ");
    write_hex_u8(transmitted);
    write_byte(b'\n');
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

fn delay_us(microseconds: u32) {
    let start = unsafe { core::ptr::read_volatile(SYSTEM_TIMER_LOW) };
    while unsafe { core::ptr::read_volatile(SYSTEM_TIMER_LOW) }.wrapping_sub(start) < microseconds {
        core::hint::spin_loop();
    }
}

fn wait_for_reset() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[alloc_error_handler]
fn allocation_error(_: Layout) -> ! {
    fail(b"NET10 ALLOC FAILED");
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    fail(b"NET10 PANIC");
}
