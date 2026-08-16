use gabix_kernel::device::FrameDevice;
use gabix_kernel::net::dhcp::{DhcpClient, ACK, CLIENT_PORT, OFFER, SERVER_PORT};
use gabix_kernel::net::stack::{NetworkInterface, MAX_FRAME_LEN};

pub enum NetworkEvent {
    None,
    Configured,
    ReplyTransmitted(bool),
}

pub struct DhcpSession {
    interface: NetworkInterface,
    client: DhcpClient,
    offered_address: Option<[u8; 4]>,
}

impl DhcpSession {
    pub const fn new(mac: [u8; 6], transaction_id: u32) -> Self {
        Self {
            interface: NetworkInterface::new(mac, [0; 4]),
            client: DhcpClient::new(mac, transaction_id),
            offered_address: None,
        }
    }

    pub fn send_discover(&self, device: &mut impl FrameDevice) -> bool {
        let mut payload = [0u8; 300];
        let Some(payload_len) = self.client.write_discover(&mut payload) else {
            return false;
        };
        self.send_dhcp(device, &payload[..payload_len])
    }

    pub fn handle_frame(&mut self, device: &mut impl FrameDevice, bytes: &[u8]) -> NetworkEvent {
        let mut reply = [0u8; MAX_FRAME_LEN];
        if let Some(length) = self.interface.reply(bytes, &mut reply) {
            return NetworkEvent::ReplyTransmitted(device.transmit(&reply[..length]));
        }

        let Some(datagram) = self.interface.receive_udp(bytes) else {
            return NetworkEvent::None;
        };
        if datagram.source_port != SERVER_PORT || datagram.destination_port != CLIENT_PORT {
            return NetworkEvent::None;
        }
        let Some(message) = self.client.parse_reply(datagram.payload) else {
            return NetworkEvent::None;
        };
        if message.message_type == OFFER {
            self.offered_address = Some(message.address);
            let mut payload = [0u8; 300];
            if let Some(payload_len) = self.client.write_request(&mut payload, &message) {
                self.send_dhcp(device, &payload[..payload_len]);
            }
        } else if message.message_type == ACK && self.offered_address == Some(message.address) {
            self.interface.configure(message.address);
            return NetworkEvent::Configured;
        }
        NetworkEvent::None
    }

    pub fn is_configured(&self) -> bool {
        self.interface.address() != [0; 4]
    }

    fn send_dhcp(&self, device: &mut impl FrameDevice, payload: &[u8]) -> bool {
        let mut frame = [0u8; MAX_FRAME_LEN];
        if let Some(length) = self.interface.write_udp_from(
            [0; 4],
            [0xff; 6],
            [255; 4],
            CLIENT_PORT,
            SERVER_PORT,
            payload,
            &mut frame,
        ) {
            device.transmit(&frame[..length])
        } else {
            false
        }
    }
}
