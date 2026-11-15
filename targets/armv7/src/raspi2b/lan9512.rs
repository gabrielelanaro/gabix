//! LAN9512 register commands use USB control transfers; Ethernet frames use
//! the bulk endpoints. Field definitions follow Linux smsc95xx.h/.c.

use super::usb::{Dwc2, UsbDevice};
use gabix_kernel::device::{EthernetFrame, FrameDevice, MAX_ETHERNET_FRAME_LEN};
use gabix_kernel::net::smsc95xx::{read_rx_frame, write_tx_frame, RX_HEADER_LEN, TX_HEADER_LEN};

const USB_VENDOR_DEVICE_READ: u8 = 0xc0;
const USB_VENDOR_DEVICE_WRITE: u8 = 0x40;
const UNUSED_REQUEST_VALUE: u16 = 0;
const MII_PHY_ADDRESS_SHIFT: u32 = 11;
const MII_REGISTER_SHIFT: u32 = 6;
const MII_REGISTER_MASK: u32 = 0x1f;
const CHECKSUM_OFFLOAD_DISABLED: u32 = 0;
const CLEAR_ALL_INTERRUPT_STATUS: u32 = u32::MAX;
const FALLBACK_MAC: [u8; 6] = [0x02, 0x47, 0x41, 0x42, 0x49, 0x58];

const DATA0: u32 = 0;
const DATA1: u32 = 2;

const WRITE_REGISTER: u8 = 0xa0;
const READ_REGISTER: u8 = 0xa1;
const INT_STS: u16 = 0x08;
const TX_CFG: u16 = 0x10;
const HW_CFG: u16 = 0x14;
const AFC_CFG: u16 = 0x2c;
const E2P_CMD: u16 = 0x30;
const E2P_DATA: u16 = 0x34;
const BURST_CAP: u16 = 0x38;
const MAC_CR: u16 = 0x100;
const MII_ADDR: u16 = 0x114;
const MII_DATA: u16 = 0x118;
const ADDRH: u16 = 0x104;
const ADDRL: u16 = 0x108;
const COE_CR: u16 = 0x130;

const TX_CFG_ON: u32 = 0x0000_0004;
const BULK_IN_EMPTY_RESPONSE: u32 = 0x0000_1000;
const RX_DATA_OFFSET_MASK: u32 = 0x0000_0600;
const LITE_RESET_REQUEST: u32 = 0x0000_0008;
// Linux reference flow-control thresholds and backpressure configuration.
const AFC_CFG_DEFAULT: u32 = 0x00f8_30a1;
const MAC_CR_FULL_DUPLEX: u32 = 0x0010_0000;
const MAC_CR_TX_ENABLE: u32 = 0x0000_0008;
const MAC_CR_RX_ENABLE: u32 = 0x0000_0004;
const E2P_BUSY: u32 = 0x8000_0000;
const E2P_TIMEOUT: u32 = 0x0000_0400;
const E2P_READ: u32 = 0;
const MII_BUSY: u32 = 1;
const INTERNAL_PHY_ID: u32 = 1;
const MII_BMSR: u32 = 1;
const BMSR_LINK_STATUS: u16 = 1 << 2;
const EEPROM_MAC_OFFSET: u32 = 1;
const REGISTER_POLL_LIMIT: usize = 1000;
const FCS_LEN: usize = 4;

pub struct Lan9512Ethernet {
    host: Dwc2,
    device: UsbDevice,
    bulk_in_endpoint: u8,
    bulk_out_endpoint: u8,
    bulk_max_packet: u16,
    mac: [u8; 6],
    tx_pid: u32,
    rx_pid: u32,
}

impl Lan9512Ethernet {
    pub unsafe fn configure(mut host: Dwc2, device: UsbDevice) -> Option<Self> {
        // Configure USB endpoints, then wait for the adapter's internal reset.
        let endpoints = host.configure_bulk_device(device)?;
        write_register(&mut host, device, HW_CFG, LITE_RESET_REQUEST)?;
        wait_register_clear(&mut host, device, HW_CFG, LITE_RESET_REQUEST)?;

        // Use EEPROM identity when valid; otherwise use our local unicast MAC.
        let mac = read_mac_address(&mut host, device)
            .filter(valid_mac_address)
            .unwrap_or(FALLBACK_MAC);
        write_register(
            &mut host,
            device,
            ADDRL,
            u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]),
        )?;
        write_register(
            &mut host,
            device,
            ADDRH,
            u16::from_le_bytes([mac[4], mac[5]]) as u32,
        )?;

        // Zero RX data offset places frame bytes immediately after RX status.
        let hardware = read_register(&mut host, device, HW_CFG)?;
        write_register(
            &mut host,
            device,
            HW_CFG,
            (hardware | BULK_IN_EMPTY_RESPONSE) & !RX_DATA_OFFSET_MASK,
        )?;
        write_register(&mut host, device, BURST_CAP, 0)?;
        write_register(&mut host, device, INT_STS, CLEAR_ALL_INTERRUPT_STATUS)?;
        write_register(&mut host, device, AFC_CFG, AFC_CFG_DEFAULT)?;
        write_register(&mut host, device, COE_CR, CHECKSUM_OFFLOAD_DISABLED)?;

        let mac_control = read_register(&mut host, device, MAC_CR)?;
        write_register(
            &mut host,
            device,
            MAC_CR,
            mac_control | MAC_CR_FULL_DUPLEX | MAC_CR_TX_ENABLE | MAC_CR_RX_ENABLE,
        )?;
        write_register(&mut host, device, TX_CFG, TX_CFG_ON)?;

        Some(Self {
            host,
            device,
            bulk_in_endpoint: endpoints.input,
            bulk_out_endpoint: endpoints.output,
            bulk_max_packet: endpoints.max_packet_size,
            mac,
            tx_pid: DATA0,
            rx_pid: DATA0,
        })
    }

    fn update_pid(pid: &mut u32, length: usize, packet_size: usize) {
        // USB DATA0/DATA1 alternates per packet. An odd packet count changes
        // the starting PID for the next transfer; a zero-length packet counts.
        let packets = length.max(1).div_ceil(packet_size);
        if packets & 1 != 0 {
            *pid = if *pid == DATA0 { DATA1 } else { DATA0 };
        }
    }

    pub unsafe fn transmit_frame(&mut self, frame: &[u8]) -> bool {
        let mut bytes = [0u8; TX_HEADER_LEN + MAX_ETHERNET_FRAME_LEN];
        let Some(length) = write_tx_frame(frame, &mut bytes) else {
            return false;
        };
        if self
            .host
            .bulk_out(
                self.device,
                self.bulk_out_endpoint,
                self.bulk_max_packet,
                self.tx_pid,
                &bytes[..length],
            )
            .is_none()
        {
            return false;
        }
        Self::update_pid(&mut self.tx_pid, length, self.bulk_max_packet as usize);
        true
    }

    pub unsafe fn link_up(&mut self) -> Option<bool> {
        // BMSR link status is latch-low, so Linux reads it twice as well.
        let _ = read_phy_register(&mut self.host, self.device, MII_BMSR)?;
        Some(read_phy_register(&mut self.host, self.device, MII_BMSR)? & BMSR_LINK_STATUS != 0)
    }
}

unsafe impl Send for Lan9512Ethernet {}

impl FrameDevice for Lan9512Ethernet {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn transmit(&mut self, frame: &[u8]) -> bool {
        unsafe { self.transmit_frame(frame) }
    }

    fn receive(&mut self) -> Option<EthernetFrame> {
        let mut bytes = [0u8; RX_HEADER_LEN + MAX_ETHERNET_FRAME_LEN + FCS_LEN];
        let length = unsafe {
            self.host.bulk_in(
                self.device,
                self.bulk_in_endpoint,
                self.bulk_max_packet,
                self.rx_pid,
                &mut bytes,
            )?
        };
        Self::update_pid(&mut self.rx_pid, length, self.bulk_max_packet as usize);
        let frame = read_rx_frame(&bytes[..length])?;
        Some(EthernetFrame::from_bytes(frame))
    }
}

unsafe fn read_register(host: &mut Dwc2, device: UsbDevice, register: u16) -> Option<u32> {
    let mut bytes = [0u8; 4];
    if host.control_read(
        device,
        USB_VENDOR_DEVICE_READ,
        READ_REGISTER,
        UNUSED_REQUEST_VALUE,
        register,
        &mut bytes,
    )? != bytes.len()
    {
        return None;
    }
    Some(u32::from_le_bytes(bytes))
}

unsafe fn write_register(
    host: &mut Dwc2,
    device: UsbDevice,
    register: u16,
    value: u32,
) -> Option<()> {
    host.control_write(
        device,
        USB_VENDOR_DEVICE_WRITE,
        WRITE_REGISTER,
        UNUSED_REQUEST_VALUE,
        register,
        &value.to_le_bytes(),
    )
}

unsafe fn read_phy_register(host: &mut Dwc2, device: UsbDevice, register: u32) -> Option<u16> {
    // The MAC proxies access to the Ethernet physical-layer (PHY) registers.
    // Wait for the previous operation before submitting this read.
    wait_register_clear(host, device, MII_ADDR, MII_BUSY)?;
    write_register(
        host,
        device,
        MII_ADDR,
        INTERNAL_PHY_ID << MII_PHY_ADDRESS_SHIFT
            | (register & MII_REGISTER_MASK) << MII_REGISTER_SHIFT
            | MII_BUSY,
    )?;
    wait_register_clear(host, device, MII_ADDR, MII_BUSY)?;
    Some(read_register(host, device, MII_DATA)? as u16)
}

unsafe fn wait_register_clear(
    host: &mut Dwc2,
    device: UsbDevice,
    register: u16,
    mask: u32,
) -> Option<()> {
    // Each poll is a USB control read; the limit counts attempts, not time.
    for _ in 0..REGISTER_POLL_LIMIT {
        if read_register(host, device, register)? & mask == 0 {
            return Some(());
        }
    }
    None
}

unsafe fn read_mac_address(host: &mut Dwc2, device: UsbDevice) -> Option<[u8; 6]> {
    wait_register_clear(host, device, E2P_CMD, E2P_BUSY)?;
    let mut mac = [0u8; 6];
    for (offset, byte) in mac.iter_mut().enumerate() {
        write_register(
            host,
            device,
            E2P_CMD,
            E2P_BUSY | E2P_READ | (EEPROM_MAC_OFFSET + offset as u32),
        )?;
        wait_register_clear(host, device, E2P_CMD, E2P_BUSY)?;
        if read_register(host, device, E2P_CMD)? & E2P_TIMEOUT != 0 {
            return None;
        }
        *byte = read_register(host, device, E2P_DATA)? as u8;
    }
    Some(mac)
}

fn valid_mac_address(mac: &[u8; 6]) -> bool {
    mac.iter().any(|byte| *byte != 0) && mac.iter().any(|byte| *byte != 0xff) && mac[0] & 1 == 0
}
