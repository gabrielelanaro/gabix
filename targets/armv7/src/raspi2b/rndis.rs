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
const PACKET_FILTER: u32 = 1 | 2 | 8;
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

        let mut initialize = [0u8; 24];
        put_u32(&mut initialize, 0, RNDIS_INITIALIZE);
        put_u32(&mut initialize, 4, 24);
        put_u32(&mut initialize, 8, 1);
        put_u32(&mut initialize, 12, 1);
        put_u32(&mut initialize, 16, 0);
        put_u32(&mut initialize, 20, 2048);
        send_command(&mut host, device, &initialize)?;
        let mut response = [0u8; 256];
        let response_len = receive_response(&mut host, device, &mut response)?;
        if response_len < 16
            || get_u32(&response, 0) != RNDIS_INITIALIZE_COMPLETE
            || get_u32(&response, 12) != RNDIS_STATUS_SUCCESS
        {
            return None;
        }

        let mut set_filter = [0u8; 32];
        put_u32(&mut set_filter, 0, RNDIS_SET);
        put_u32(&mut set_filter, 4, 32);
        put_u32(&mut set_filter, 8, 2);
        put_u32(&mut set_filter, 12, OID_GEN_CURRENT_PACKET_FILTER);
        put_u32(&mut set_filter, 16, 4);
        put_u32(&mut set_filter, 20, 20);
        put_u32(&mut set_filter, 28, PACKET_FILTER);
        send_command(&mut host, device, &set_filter)?;
        let response_len = receive_response(&mut host, device, &mut response)?;
        if response_len < 16
            || get_u32(&response, 0) != RNDIS_SET_COMPLETE
            || get_u32(&response, 12) != RNDIS_STATUS_SUCCESS
        {
            return None;
        }

        Some(Self {
            host,
            device,
            bulk_in_endpoint: endpoints.input,
            bulk_out_endpoint: endpoints.output,
            bulk_max_packet: endpoints.max_packet_size,
            mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            tx_pid: DATA0,
            rx_pid: DATA0,
        })
    }

    fn update_pid(pid: &mut u32, length: usize, packet_size: usize) {
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
        let frame_len = frame.len().max(60);
        let message_len = RNDIS_HEADER_LEN + frame_len;
        let mut message = [0u8; RNDIS_HEADER_LEN + MAX_ETHERNET_FRAME_LEN];
        put_u32(&mut message, 0, RNDIS_PACKET);
        put_u32(&mut message, 4, message_len as u32);
        put_u32(&mut message, 8, (RNDIS_HEADER_LEN - 8) as u32);
        put_u32(&mut message, 12, frame_len as u32);
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
        if message_len < RNDIS_HEADER_LEN || get_u32(&message, 0) != RNDIS_PACKET {
            return None;
        }
        Self::update_pid(&mut self.rx_pid, message_len, self.bulk_max_packet as usize);
        let data_offset = get_u32(&message, 8) as usize + 8;
        let data_len = get_u32(&message, 12) as usize;
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
    host.control_write(device, 0x21, 0, 0, 0, command)
}

unsafe fn receive_response(
    host: &mut Dwc2,
    device: UsbDevice,
    response: &mut [u8],
) -> Option<usize> {
    host.control_read(device, 0xa1, 1, 0, 0, response)
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
