//! Remote NDIS (Network Driver Interface Specification) over USB for QEMU.
//! Control requests initialize the adapter; bulk transfers carry a 44-byte
//! RNDIS packet header followed by Ethernet bytes. Integers are little-endian.
//! This adapter uses the fixed MAC configured by the QEMU test scripts.

use super::usb::{Dwc2, UsbDevice};
use gabix_kernel::device::{EthernetFrame, FrameDevice, MAX_ETHERNET_FRAME_LEN};

const DATA0: u32 = 0;
const DATA1: u32 = 2;
const RNDIS_PACKET: u32 = 1;
const RNDIS_INITIALIZE: u32 = 2;
const RNDIS_SET: u32 = 5;
const RNDIS_INITIALIZE_COMPLETE: u32 = 0x8000_0002;
const RNDIS_SET_COMPLETE: u32 = 0x8000_0005;
const RNDIS_STATUS_SUCCESS: u32 = 0;
const OID_GEN_CURRENT_PACKET_FILTER: u32 = 0x0001_010e;
const FILTER_DIRECTED: u32 = 1;
const FILTER_MULTICAST: u32 = 2;
const FILTER_BROADCAST: u32 = 8;
const PACKET_FILTER: u32 = FILTER_DIRECTED | FILTER_MULTICAST | FILTER_BROADCAST;
const INITIALIZE_BYTES: usize = 24;
const SET_FILTER_BYTES: usize = 32;
const COMPLETION_PREFIX_BYTES: usize = 16;
const RESPONSE_BUFFER_BYTES: usize = 256;
const ADVERTISED_MAX_TRANSFER: u32 = 2048;
const INITIALIZE_REQUEST_ID: u32 = 1;
const FILTER_REQUEST_ID: u32 = 2;
const RNDIS_MAJOR_VERSION: u32 = 1;
const RNDIS_MINOR_VERSION: u32 = 0;
const MIN_ETHERNET_BYTES_WITHOUT_FCS: usize = 60;
const QEMU_TEST_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
const USB_CLASS_INTERFACE_WRITE: u8 = 0x21;
const USB_CLASS_INTERFACE_READ: u8 = 0xa1;
const SEND_ENCAPSULATED_COMMAND: u8 = 0;
const GET_ENCAPSULATED_RESPONSE: u8 = 1;
const CONTROL_INTERFACE: u16 = 0;
const UNUSED_CONTROL_VALUE: u16 = 0;

mod field {
    pub const MESSAGE_TYPE: usize = 0;
    pub const MESSAGE_LENGTH: usize = 4;
    pub const REQUEST_ID: usize = 8;
    pub const COMPLETION_STATUS: usize = 12;
    pub const MAJOR_VERSION: usize = 12;
    pub const MINOR_VERSION: usize = 16;
    pub const MAX_TRANSFER: usize = 20;
    pub const OID: usize = 12;
    pub const INFO_LENGTH: usize = 16;
    pub const INFO_OFFSET: usize = 20;
    pub const FILTER_VALUE: usize = 28;
    pub const DATA_OFFSET: usize = 8;
    pub const DATA_LENGTH: usize = 12;
}
const RNDIS_HEADER_LEN: usize = 44;

pub struct RndisEthernet {
    host: Dwc2,
    device: UsbDevice,
    bulk_in_endpoint: u8,
    bulk_out_endpoint: u8,
    bulk_max_packet: u16,
    mac: [u8; 6],
    tx_pid: u32,
    rx_pid: u32,
}

impl RndisEthernet {
    pub unsafe fn configure(mut host: Dwc2, device: UsbDevice) -> Option<Self> {
        let endpoints = host.configure_bulk_device(device)?;

        let mut initialize = [0u8; INITIALIZE_BYTES];
        put_u32(&mut initialize, field::MESSAGE_TYPE, RNDIS_INITIALIZE);
        put_u32(
            &mut initialize,
            field::MESSAGE_LENGTH,
            INITIALIZE_BYTES as u32,
        );
        put_u32(&mut initialize, field::REQUEST_ID, INITIALIZE_REQUEST_ID);
        put_u32(&mut initialize, field::MAJOR_VERSION, RNDIS_MAJOR_VERSION);
        put_u32(&mut initialize, field::MINOR_VERSION, RNDIS_MINOR_VERSION);
        put_u32(
            &mut initialize,
            field::MAX_TRANSFER,
            ADVERTISED_MAX_TRANSFER,
        );
        send_command(&mut host, device, &initialize)?;
        let mut response = [0u8; RESPONSE_BUFFER_BYTES];
        let response_len = receive_response(&mut host, device, &mut response)?;
        if response_len < COMPLETION_PREFIX_BYTES
            || get_u32(&response, field::MESSAGE_TYPE) != RNDIS_INITIALIZE_COMPLETE
            || get_u32(&response, field::COMPLETION_STATUS) != RNDIS_STATUS_SUCCESS
        {
            return None;
        }

        // Accept directed, multicast and broadcast traffic. The information
        // offset is measured from RequestId, eight bytes into the message.
        let mut set_filter = [0u8; SET_FILTER_BYTES];
        put_u32(&mut set_filter, field::MESSAGE_TYPE, RNDIS_SET);
        put_u32(
            &mut set_filter,
            field::MESSAGE_LENGTH,
            SET_FILTER_BYTES as u32,
        );
        put_u32(&mut set_filter, field::REQUEST_ID, FILTER_REQUEST_ID);
        put_u32(&mut set_filter, field::OID, OID_GEN_CURRENT_PACKET_FILTER);
        put_u32(&mut set_filter, field::INFO_LENGTH, 4);
        put_u32(
            &mut set_filter,
            field::INFO_OFFSET,
            (field::FILTER_VALUE - field::REQUEST_ID) as u32,
        );
        put_u32(&mut set_filter, field::FILTER_VALUE, PACKET_FILTER);
        send_command(&mut host, device, &set_filter)?;
        let response_len = receive_response(&mut host, device, &mut response)?;
        if response_len < COMPLETION_PREFIX_BYTES
            || get_u32(&response, field::MESSAGE_TYPE) != RNDIS_SET_COMPLETE
            || get_u32(&response, field::COMPLETION_STATUS) != RNDIS_STATUS_SUCCESS
        {
            return None;
        }

        Some(Self {
            host,
            device,
            bulk_in_endpoint: endpoints.input,
            bulk_out_endpoint: endpoints.output,
            bulk_max_packet: endpoints.max_packet_size,
            mac: QEMU_TEST_MAC,
            tx_pid: DATA0,
            rx_pid: DATA0,
        })
    }

    fn update_pid(pid: &mut u32, length: usize, packet_size: usize) {
        // Each USB packet toggles DATA0/DATA1, including a zero-length packet.
        let packets = length.max(1).div_ceil(packet_size);
        if packets & 1 != 0 {
            *pid = if *pid == DATA0 { DATA1 } else { DATA0 };
        }
    }
}

unsafe impl Send for RndisEthernet {}

impl FrameDevice for RndisEthernet {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn transmit(&mut self, frame: &[u8]) -> bool {
        if frame.len() > MAX_ETHERNET_FRAME_LEN {
            return false;
        }
        let frame_len = frame.len().max(MIN_ETHERNET_BYTES_WITHOUT_FCS);
        let message_len = RNDIS_HEADER_LEN + frame_len;
        let mut message = [0u8; RNDIS_HEADER_LEN + MAX_ETHERNET_FRAME_LEN];
        put_u32(&mut message, field::MESSAGE_TYPE, RNDIS_PACKET);
        put_u32(&mut message, field::MESSAGE_LENGTH, message_len as u32);
        put_u32(
            &mut message,
            field::DATA_OFFSET,
            (RNDIS_HEADER_LEN - field::DATA_OFFSET) as u32,
        );
        put_u32(&mut message, field::DATA_LENGTH, frame_len as u32);
        message[RNDIS_HEADER_LEN..RNDIS_HEADER_LEN + frame.len()].copy_from_slice(frame);
        unsafe {
            if self
                .host
                .bulk_out(
                    self.device,
                    self.bulk_out_endpoint,
                    self.bulk_max_packet,
                    self.tx_pid,
                    &message[..message_len],
                )
                .is_some()
            {
                Self::update_pid(&mut self.tx_pid, message_len, self.bulk_max_packet as usize);
                true
            } else {
                false
            }
        }
    }

    fn receive(&mut self) -> Option<EthernetFrame> {
        let mut message = [0u8; RNDIS_HEADER_LEN + MAX_ETHERNET_FRAME_LEN];
        let message_len = unsafe {
            self.host.bulk_in(
                self.device,
                self.bulk_in_endpoint,
                self.bulk_max_packet,
                self.rx_pid,
                &mut message,
            )?
        };
        if message_len < RNDIS_HEADER_LEN || get_u32(&message, field::MESSAGE_TYPE) != RNDIS_PACKET
        {
            return None;
        }
        Self::update_pid(&mut self.rx_pid, message_len, self.bulk_max_packet as usize);
        // DataOffset is relative to its own field, not the message beginning.
        let data_offset = get_u32(&message, field::DATA_OFFSET) as usize + field::DATA_OFFSET;
        let data_len = get_u32(&message, field::DATA_LENGTH) as usize;
        let data_end = data_offset.checked_add(data_len)?;
        if data_len == 0
            || data_len > MAX_ETHERNET_FRAME_LEN
            || data_end > message_len
            || data_offset < RNDIS_HEADER_LEN
        {
            return None;
        }
        Some(EthernetFrame::from_bytes(&message[data_offset..data_end]))
    }
}

unsafe fn send_command(host: &mut Dwc2, device: UsbDevice, command: &[u8]) -> Option<()> {
    host.control_write(
        device,
        USB_CLASS_INTERFACE_WRITE,
        SEND_ENCAPSULATED_COMMAND,
        UNUSED_CONTROL_VALUE,
        CONTROL_INTERFACE,
        command,
    )
}

unsafe fn receive_response(
    host: &mut Dwc2,
    device: UsbDevice,
    response: &mut [u8],
) -> Option<usize> {
    host.control_read(
        device,
        USB_CLASS_INTERFACE_READ,
        GET_ENCAPSULATED_RESPONSE,
        UNUSED_CONTROL_VALUE,
        CONTROL_INTERFACE,
        response,
    )
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
