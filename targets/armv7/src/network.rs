//! Frame-driven DHCP lease lifetime for the ARM network service.
//! The caller polls with monotonic seconds and delivers incoming Ethernet
//! frames. This object changes the interface address only after accepting a
//! matching server reply; retries and lease expiry advance even without replies.

use gabix_kernel::device::FrameDevice;
use gabix_kernel::net::dhcp::{DhcpClient, DhcpReply, ACK, CLIENT_PORT, NAK, OFFER, SERVER_PORT};
use gabix_kernel::net::stack::{NetworkInterface, MAX_FRAME_LEN};

// Client retry policy. Discovery/request back off exponentially; renewal and
// rebinding retry halfway to their next deadline with a 60-second floor.
const INITIAL_RETRY_SECONDS: u64 = 4;
const MAX_DISCOVERY_RETRY_SECONDS: u64 = 64;
const MIN_LEASE_RETRY_SECONDS: u64 = 60;
const DHCP_PACKET_BUFFER_BYTES: usize = 320;
const UNSPECIFIED_IP: [u8; 4] = [0; 4];
const BROADCAST_IP: [u8; 4] = [255; 4];
const BROADCAST_MAC: [u8; 6] = [0xff; 6];
const FIRST_MULTICAST_OCTET: u8 = 224;
const LOOPBACK_OCTET: u8 = 127;

pub enum NetworkEvent {
    None,
    Configured,
    ReplyTransmitted(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum LeaseState {
    /// Broadcast Discover and wait for a usable offer.
    Selecting,
    /// Request the selected offer and wait for its server to acknowledge.
    Requesting,
    /// Use the accepted address until renewal becomes due.
    Bound,
    /// Ask the original server to extend the lease.
    Renewing,
    /// Broadcast a renewal request so another server can answer.
    Rebinding,
}

pub struct DhcpSession {
    interface: NetworkInterface,
    mdns: gabix_kernel::net::mdns::Host,
    client: DhcpClient,
    offer: Option<DhcpReply>,
    pub lease: Option<DhcpReply>,
    pub state: LeaseState,
    server_mac: [u8; 6],
    now: u64,
    next_retry: u64,
    retry_delay: u64,
    renew_at: u64,
    rebind_at: u64,
    expires_at: u64,
    pub attempts: u32,
    pub tx_failures: u32,
    pub received_frames: u32,
    pub rejected_dhcp: u32,
    pub lease_losses: u32,
    pub discovery_reason: &'static str,
}

impl DhcpSession {
    pub const fn new(mac: [u8; 6], transaction_id: u32) -> Self {
        Self {
            interface: NetworkInterface::new(mac, UNSPECIFIED_IP),
            mdns: gabix_kernel::net::mdns::Host::new(),
            client: DhcpClient::new(mac, transaction_id),
            offer: None,
            lease: None,
            state: LeaseState::Selecting,
            server_mac: [0; 6],
            now: 0,
            next_retry: 0,
            retry_delay: INITIAL_RETRY_SECONDS,
            renew_at: 0,
            rebind_at: 0,
            expires_at: 0,
            attempts: 0,
            tx_failures: 0,
            received_frames: 0,
            rejected_dhcp: 0,
            lease_losses: 0,
            discovery_reason: "startup",
        }
    }

    pub fn restart(&mut self, reason: &'static str) {
        self.discovery_reason = reason;
        self.renew_at = 0;
        self.rebind_at = 0;
        self.expires_at = 0;
        if self.is_configured() {
            self.lease_losses = self.lease_losses.saturating_add(1);
        }
        self.interface.configure(UNSPECIFIED_IP);
        self.client.restart();
        self.offer = None;
        self.lease = None;
        self.state = LeaseState::Selecting;
        self.next_retry = self.now;
        self.retry_delay = INITIAL_RETRY_SECONDS;
    }

    pub fn address(&self) -> [u8; 4] {
        self.interface.address()
    }

    /// Advance name probing and announcements independently of DHCP retries.
    pub fn poll_discovery(&mut self, device: &mut impl FrameDevice, now_ms: u64) {
        self.mdns.poll(&self.interface, device, now_ms);
    }
    pub fn mac_address(&self) -> [u8; 6] {
        self.interface.mac_address()
    }
    pub fn lease_remaining(&self) -> u64 {
        self.expires_at.saturating_sub(self.now)
    }

    /// `now` is monotonic elapsed time in seconds.
    /// Advance lease deadlines even when the link is down. Retry discovery
    /// indefinitely; expiry removes the old address before another discovery.
    pub fn poll(&mut self, device: &mut impl FrameDevice, now: u64, link_up: bool) {
        self.now = now;
        if self.is_configured() && now >= self.expires_at {
            self.restart("lease expired");
        }
        if !link_up {
            return;
        }
        if self.is_configured() {
            let state = if now >= self.rebind_at {
                LeaseState::Rebinding
            } else if now >= self.renew_at {
                LeaseState::Renewing
            } else {
                LeaseState::Bound
            };
            if state != self.state {
                self.state = state;
                self.next_retry = now;
            }
            if self.state == LeaseState::Bound {
                return;
            }
        }
        if now < self.next_retry {
            return;
        }
        let mut payload = [0u8; DHCP_PACKET_BUFFER_BYTES];
        let sent = match self.state {
            LeaseState::Selecting => self.send_discover(device),
            LeaseState::Requesting => {
                let length = self
                    .offer
                    .and_then(|offer| self.client.write_request(&mut payload, &offer));
                length
                    .map(|n| self.send_dhcp(device, &payload[..n]))
                    .unwrap_or(false)
            }
            LeaseState::Renewing | LeaseState::Rebinding => {
                let length = self.client.write_renewal(&mut payload, self.address());
                length
                    .map(|n| {
                        let (mac, ip) = if self.state == LeaseState::Renewing {
                            (self.server_mac, self.lease.unwrap().server)
                        } else {
                            (BROADCAST_MAC, BROADCAST_IP)
                        };
                        self.send_dhcp_to(device, self.address(), mac, ip, &payload[..n])
                    })
                    .unwrap_or(false)
            }
            LeaseState::Bound => return,
        };
        self.attempts = self.attempts.saturating_add(1);
        if !sent {
            self.tx_failures = self.tx_failures.saturating_add(1);
        }
        if matches!(self.state, LeaseState::Renewing | LeaseState::Rebinding) {
            let deadline = if self.state == LeaseState::Renewing {
                self.rebind_at
            } else {
                self.expires_at
            };
            self.next_retry =
                now + ((deadline.saturating_sub(now)) / 2).max(MIN_LEASE_RETRY_SECONDS);
        } else {
            self.next_retry = now + self.retry_delay;
            self.retry_delay = (self.retry_delay * 2).min(MAX_DISCOVERY_RETRY_SECONDS);
            // An unanswered REQUEST must eventually allow another server's offer.
            if self.state == LeaseState::Requesting
                && self.retry_delay == MAX_DISCOVERY_RETRY_SECONDS
            {
                self.restart("request timed out");
            }
        }
    }

    pub fn send_discover(&self, device: &mut impl FrameDevice) -> bool {
        let mut payload = [0u8; DHCP_PACKET_BUFFER_BYTES];
        self.client
            .write_discover(&mut payload)
            .map(|n| self.send_dhcp(device, &payload[..n]))
            .unwrap_or(false)
    }

    pub fn handle_frame(&mut self, device: &mut impl FrameDevice, bytes: &[u8]) -> NetworkEvent {
        self.received_frames = self.received_frames.saturating_add(1);
        if self.mdns.handle(&self.interface, device, bytes) {
            return NetworkEvent::None;
        }
        let mut reply = [0u8; MAX_FRAME_LEN];
        if self.is_configured() {
            if let Some(length) = self.interface.reply(bytes, &mut reply) {
                let sent = device.transmit(&reply[..length]);
                if !sent {
                    self.tx_failures = self.tx_failures.saturating_add(1);
                }
                return NetworkEvent::ReplyTransmitted(sent);
            }
        }
        let Some(datagram) = self.interface.receive_udp(bytes) else {
            return NetworkEvent::None;
        };
        if datagram.source_port != SERVER_PORT || datagram.destination_port != CLIENT_PORT {
            return NetworkEvent::None;
        }
        let Some(message) = self.client.parse_reply(datagram.payload) else {
            self.rejected_dhcp = self.rejected_dhcp.saturating_add(1);
            return NetworkEvent::None;
        };
        if message.message_type == OFFER
            && self.state == LeaseState::Selecting
            && usable_address(message.address)
            && usable_address(message.server)
        {
            self.offer = Some(message);
            self.state = LeaseState::Requesting;
            self.retry_delay = INITIAL_RETRY_SECONDS;
            self.next_retry = self.now + INITIAL_RETRY_SECONDS;
            let mut payload = [0u8; DHCP_PACKET_BUFFER_BYTES];
            if let Some(n) = self.client.write_request(&mut payload, &message) {
                self.attempts = self.attempts.saturating_add(1);
                if !self.send_dhcp(device, &payload[..n]) {
                    self.tx_failures = self.tx_failures.saturating_add(1);
                }
            }
        } else {
            let selected_server = self.offer.or(self.lease).map(|lease| lease.server);
            let server_matches =
                self.state == LeaseState::Rebinding || selected_server == Some(message.server);
            if message.message_type == NAK && server_matches && self.state != LeaseState::Selecting
            {
                self.restart("server rejected lease");
            } else if message.message_type == ACK && server_matches && message.lease_seconds > 0 {
                let expected = if self.state == LeaseState::Requesting {
                    self.offer.map(|o| o.address)
                } else if matches!(self.state, LeaseState::Renewing | LeaseState::Rebinding) {
                    Some(self.address())
                } else {
                    None
                };
                let address = if message.address == UNSPECIFIED_IP {
                    self.address()
                } else {
                    message.address
                };
                if expected != Some(address) || !usable_address(address) {
                    self.rejected_dhcp += 1;
                    return NetworkEvent::None;
                }
                let mut lease = message;
                lease.address = address;
                let duration = lease.lease_seconds as u64;
                // RFC 2131: T1 starts renewal halfway through the lease;
                // T2 starts rebinding at seven eighths. Preserve the existing
                // one-second minimum for this client's short-lease handling.
                let default_t1 = (duration / 2).max(1);
                let default_t2 = (duration * 7 / 8).max(default_t1);
                let (t1, t2) = if lease.renewal_seconds > 0
                    && lease.renewal_seconds < lease.rebinding_seconds
                    && lease.rebinding_seconds < lease.lease_seconds
                {
                    (lease.renewal_seconds as u64, lease.rebinding_seconds as u64)
                } else {
                    (default_t1, default_t2)
                };
                self.renew_at = self.now + t1;
                self.rebind_at = self.now + t2;
                self.expires_at = self.now + duration;
                self.server_mac = datagram.source_mac;
                self.interface.configure(address);
                self.lease = Some(lease);
                self.offer = None;
                self.state = LeaseState::Bound;
                return NetworkEvent::Configured;
            } else {
                self.rejected_dhcp = self.rejected_dhcp.saturating_add(1);
            }
        }
        NetworkEvent::None
    }

    #[cfg(feature = "pi-sd-transfer")]
    pub fn handle_sd_transfer(
        &self,
        device: &mut impl FrameDevice,
        bytes: &[u8],
        receiver: &mut crate::sd_transfer::Receiver,
    ) -> bool {
        let Some(datagram) = self.interface.receive_udp(bytes) else {
            return false;
        };
        if !self.is_configured()
            || datagram.destination_ip != self.interface.address()
            || datagram.destination_port != crate::sd_transfer::PORT
        {
            return false;
        }
        let mut payload = [0; 584];
        if let Some(length) = receiver.handle(
            (datagram.source_ip, datagram.source_port),
            datagram.payload,
            &mut payload,
        ) {
            let mut frame = [0; MAX_FRAME_LEN];
            if let Some(frame_length) = self.interface.write_udp(
                datagram.source_mac,
                datagram.source_ip,
                crate::sd_transfer::PORT,
                datagram.source_port,
                &payload[..length],
                &mut frame,
            ) {
                receiver.transmitted(device.transmit(&frame[..frame_length]));
            }
        }
        true
    }

    pub fn is_configured(&self) -> bool {
        self.interface.address() != UNSPECIFIED_IP
    }

    fn send_dhcp(&self, device: &mut impl FrameDevice, payload: &[u8]) -> bool {
        self.send_dhcp_to(device, UNSPECIFIED_IP, BROADCAST_MAC, BROADCAST_IP, payload)
    }
    fn send_dhcp_to(
        &self,
        device: &mut impl FrameDevice,
        source: [u8; 4],
        mac: [u8; 6],
        ip: [u8; 4],
        payload: &[u8],
    ) -> bool {
        let mut frame = [0u8; MAX_FRAME_LEN];
        self.interface
            .write_udp_from(
                source,
                mac,
                ip,
                CLIENT_PORT,
                SERVER_PORT,
                payload,
                &mut frame,
            )
            .map(|n| device.transmit(&frame[..n]))
            .unwrap_or(false)
    }
}
fn usable_address(address: [u8; 4]) -> bool {
    // This filter excludes zero-network, multicast/reserved, and loopback
    // addresses. It does not validate a subnet's usable host range.
    address[0] != 0 && address[0] < FIRST_MULTICAST_OCTET && address[0] != LOOPBACK_OCTET
}
