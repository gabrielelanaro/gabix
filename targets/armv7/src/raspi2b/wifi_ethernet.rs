//! A Wi-Fi station exposes Ethernet frames after association and WPA2 complete.
//! Polling advances connection state, so UART and network management keep running
//! while the access point is absent. Credentials never appear in status events.
use super::{
    random,
    rtl8188cu::{Error, Identity, Rtl8188cu},
    usb::{Dwc2, UsbDevice},
};
use gabix_kernel::{
    device::{EthernetFrame, FrameDevice, MAX_ETHERNET_FRAME_LEN},
    net::{
        wifi::{self, CcmpLink},
        wpa2,
    },
};

// Connection policy, measured against the Pi's microsecond system timer.
// Scan channels 1..=13; each authentication/association request gets three tries.
const FIRST_SCAN_CHANNEL: u8 = 1;
const LAST_SCAN_CHANNEL: u8 = 13;
const SCAN_DWELL_US: u32 = 300_000;
const JOIN_RETRY_US: u32 = 1_000_000;
const JOIN_ATTEMPTS: u8 = 3;
const SECURITY_TIMEOUT_US: u32 = 10_000_000;
const LINK_SILENCE_TIMEOUT_US: u32 = 15_000_000;
const RECONNECT_BACKOFF_US: u32 = 2_000_000;
// Zero in our event log identifies a local timeout/error without an AP reason.
const LOCAL_DISCONNECT_REASON: u16 = 0;
const PAIRWISE_KEY_TABLE_ENTRY: u8 = 4;
const PAIRWISE_KEY_ID: u8 = 0;
const EAPOL_ETHERTYPE: [u8; 2] = [0x88, 0x8e];
// EAPOL header (4 bytes) + descriptor type (1 byte), then key-information flags.
const EAPOL_KEY_INFORMATION: core::ops::Range<usize> = 5..7;
const PAIRWISE_MESSAGE_1_FLAGS: [u8; 2] = [0, 0x8a];
const SYSTEM_TIMER_LOW: *const u32 = 0x3f00_3004 as *const u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Listen for a beacon advertising the configured network.
    Scanning,
    /// Open-system station authentication, before password verification.
    Authenticating,
    /// Ask the access point to accept our station and security capabilities.
    Associating,
    /// Exchange WPA2 messages and install the negotiated packet keys.
    Securing,
    /// Encrypted frames are available to the existing DHCP/IP stack.
    Online,
    /// Pause before discarding old keys and starting the scan again.
    Backoff,
}
#[derive(Clone, Copy, Debug)]
pub enum Event {
    State(State),
    Disconnected(u16),
    Hardware(Error),
    Rejected(wpa2::Error),
    GroupKeyUpdated,
}
#[derive(Clone, Copy)]
struct AccessPoint {
    address: [u8; 6],
    capabilities: u16,
}

pub struct WifiEthernet {
    radio: Rtl8188cu,
    identity: Identity,
    ssid: &'static [u8],
    pmk: &'static [u8; 32],
    link: Option<CcmpLink>,
    supplicant: Option<wpa2::Supplicant>,
    access_point: Option<AccessPoint>,
    state: State,
    entered: u32,
    // Refreshed by a matching beacon or an accepted encrypted data frame.
    last_beacon: u32,
    channel: u8,
    attempts: u8,
    sequence: u16,
    event: Option<Event>,
    pub connections: u32,
}
impl WifiEthernet {
    pub unsafe fn configure(
        host: Dwc2,
        device: UsbDevice,
        ssid: &'static [u8],
        pmk: &'static [u8; 32],
        firmware: &[u8],
    ) -> Result<Self, Error> {
        if ssid.is_empty() || ssid.len() > wifi::MAX_NETWORK_NAME_BYTES {
            return Err(Error::Configuration);
        }
        // `radio` owns the Pi USB transport and the selected dongle address.
        // These calls send USB commands; the Pi CPU executes the Rust code.
        let mut radio = Rtl8188cu::attach(host, device)?;
        // Read the factory identity and per-device calibration values.
        let identity = radio.identify()?;
        // A kernel reboot can leave the dongle running. Reset its internal
        // power state before applying the startup sequence again.
        radio.power_off()?;
        radio.power_on()?;
        // Allocate the dongle's packet buffers and connect its USB queues.
        radio.initialize_packet_memory()?;
        // Copy the vendor program into the dongle's processor memory.
        radio.load_firmware(firmware)?;
        // Configure and calibrate the radio so scanning can receive beacons.
        radio.initialize_receiver(&identity)?;
        let now = clock();
        Ok(Self {
            radio,
            identity,
            ssid,
            pmk,
            link: None,
            supplicant: None,
            access_point: None,
            state: State::Scanning,
            entered: now,
            last_beacon: now,
            channel: FIRST_SCAN_CHANNEL,
            attempts: 0,
            sequence: 1,
            event: Some(Event::State(State::Scanning)),
            connections: 0,
        })
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn link_up(&self) -> bool {
        self.state == State::Online
    }
    pub fn take_event(&mut self) -> Option<Event> {
        self.event.take()
    }

    /// Disconnect this station and start a fresh handshake with a fresh nonce.
    /// Used for local recovery as well as the physical reconnect check.
    pub fn reconnect(&mut self) {
        if let Some(ap) = self.access_point {
            let mut frame = [0u8; wifi::MANAGEMENT_HEADER_BYTES + 2];
            frame[0] = wifi::DEAUTHENTICATION;
            frame[4..10].copy_from_slice(&ap.address);
            frame[10..16].copy_from_slice(&self.identity.mac);
            frame[16..22].copy_from_slice(&ap.address);
            frame[24..26].copy_from_slice(&wifi::REASON_STATION_LEAVING.to_le_bytes());
            let _ = self.radio.transmit(&frame);
        }
        self.disconnect(wifi::REASON_STATION_LEAVING);
    }
    fn transition(&mut self, state: State) {
        self.state = state;
        self.entered = clock();
        self.event = Some(Event::State(state));
    }
    fn disconnect(&mut self, reason: u16) {
        self.link = None;
        self.supplicant = None;
        self.access_point = None;
        self.transition(State::Backoff);
        self.event = Some(Event::Disconnected(reason));
    }
    fn hardware_failure(&mut self, error: Error) {
        self.disconnect(LOCAL_DISCONNECT_REASON);
        self.event = Some(Event::Hardware(error));
    }
    fn send_join(&mut self) -> Result<(), Error> {
        let ap = self.access_point.ok_or(Error::Configuration)?;
        let mut frame = [0u8; 256];
        let length = match self.state {
            State::Authenticating => wifi::authentication_request(
                &mut frame,
                self.identity.mac,
                ap.address,
                self.sequence,
            ),
            State::Associating => wifi::association_request(
                &mut frame,
                self.identity.mac,
                ap.address,
                self.sequence,
                self.ssid,
                ap.capabilities,
            ),
            _ => return Err(Error::Configuration),
        }
        .ok_or(Error::Configuration)?;
        self.sequence = self.sequence.wrapping_add(1);
        self.radio.transmit(&frame[..length])?;
        self.attempts += 1;
        self.entered = clock();
        Ok(())
    }
    fn advance_deadline(&mut self) -> Result<(), Error> {
        let now = clock();
        let elapsed = now.wrapping_sub(self.entered);
        match self.state {
            State::Scanning if elapsed >= SCAN_DWELL_US => {
                self.channel = self.channel % LAST_SCAN_CHANNEL + FIRST_SCAN_CHANNEL;
                self.radio.set_channel(self.channel)?;
                self.entered = now;
            }
            State::Authenticating | State::Associating if elapsed >= JOIN_RETRY_US => {
                if self.attempts >= JOIN_ATTEMPTS {
                    self.disconnect(LOCAL_DISCONNECT_REASON);
                } else {
                    self.send_join()?;
                }
            }
            State::Securing if elapsed >= SECURITY_TIMEOUT_US => {
                self.disconnect(LOCAL_DISCONNECT_REASON)
            }
            State::Online if now.wrapping_sub(self.last_beacon) >= LINK_SILENCE_TIMEOUT_US => {
                self.disconnect(LOCAL_DISCONNECT_REASON)
            }
            State::Backoff if elapsed >= RECONNECT_BACKOFF_US => {
                self.radio.clear_keys()?;
                self.channel = FIRST_SCAN_CHANNEL;
                self.radio.set_channel(FIRST_SCAN_CHANNEL)?;
                self.transition(State::Scanning);
            }
            _ => {}
        }
        Ok(())
    }
    fn handle_eapol(&mut self, packet: &[u8], encrypted: bool) -> Result<(), Error> {
        let ap = self.access_point.ok_or(Error::Configuration)?.address;
        // Full pairwise reauthentication starts with a new station nonce.
        if self.state == State::Online
            && packet.get(EAPOL_KEY_INFORMATION) == Some(&PAIRWISE_MESSAGE_1_FLAGS)
        {
            self.reconnect();
            return Ok(());
        }
        let mut reply = [0u8; 1024];
        let Some(client) = self.supplicant.as_mut() else {
            return Ok(());
        };
        let response = match client.receive(packet, &mut reply) {
            Ok(response) => response,
            Err(error) => {
                self.event = Some(Event::Rejected(error));
                return Ok(());
            }
        };
        if let Some(keys) = response.install {
            if let Some(pairwise) = keys.pairwise {
                self.radio.install_ccmp_key(
                    PAIRWISE_KEY_TABLE_ENTRY,
                    PAIRWISE_KEY_ID,
                    ap,
                    &pairwise,
                )?;
                self.link = CcmpLink::new(
                    self.identity.mac,
                    ap,
                    keys.group_id,
                    keys.group_receive_counter,
                );
            } else {
                self.link
                    .as_mut()
                    .ok_or(Error::Configuration)?
                    .install_group(keys.group_id, keys.group_receive_counter)
                    .ok_or(Error::Configuration)?;
                self.event = Some(Event::GroupKeyUpdated);
            }
            self.radio
                .install_ccmp_key(keys.group_id, keys.group_id, [0; 6], &keys.group)?;
        }
        let established = client.established();
        if encrypted {
            let mut ethernet = [0u8; 1100];
            ethernet[..6].copy_from_slice(&ap);
            ethernet[6..12].copy_from_slice(&self.identity.mac);
            ethernet[12..14].copy_from_slice(&EAPOL_ETHERTYPE);
            ethernet[14..14 + response.length].copy_from_slice(&reply[..response.length]);
            self.send_encrypted(&ethernet[..14 + response.length])?;
        } else {
            let mut frame = [0u8; 1100];
            let length = wifi::eapol_request(
                &mut frame,
                self.identity.mac,
                ap,
                self.sequence,
                &reply[..response.length],
            )
            .ok_or(Error::Configuration)?;
            self.sequence = self.sequence.wrapping_add(1);
            self.radio.transmit(&frame[..length])?;
        }
        if established && self.state != State::Online {
            self.last_beacon = clock();
            self.connections = self.connections.saturating_add(1);
            self.transition(State::Online);
        }
        Ok(())
    }
    fn send_encrypted(&mut self, ethernet: &[u8]) -> Result<(), Error> {
        let mut frame = [0u8; 2048];
        let length = self
            .link
            .as_mut()
            .and_then(|link| link.transmit(ethernet, &mut frame))
            .ok_or(Error::Configuration)?;
        self.radio.transmit(&frame[..length])
    }
    fn poll_frame(&mut self) -> Result<Option<EthernetFrame>, Error> {
        self.advance_deadline()?;
        #[repr(align(64))]
        struct Buffer([u8; 4096]);
        let mut buffer = Buffer([0; 4096]);
        let Some(received) = self.radio.receive_frame(&mut buffer.0) else {
            return Ok(None);
        };
        let frame = received.bytes;
        if let Some(beacon) = wifi::parse_beacon(frame) {
            if self.state == State::Scanning
                && beacon.ssid == self.ssid
                && beacon.rsn == Some(&wpa2::RSN[2..])
            {
                let channel = beacon.channel.unwrap_or(self.channel);
                if !(FIRST_SCAN_CHANNEL..=LAST_SCAN_CHANNEL).contains(&channel) {
                    return Ok(None);
                }
                self.radio
                    .prepare_station(&self.identity, beacon.bssid, channel)?;
                self.radio.clear_keys()?;
                self.access_point = Some(AccessPoint {
                    address: beacon.bssid,
                    capabilities: beacon.capabilities,
                });
                self.attempts = 0;
                self.transition(State::Authenticating);
                self.send_join()?;
            } else if self
                .access_point
                .is_some_and(|ap| ap.address == beacon.bssid)
            {
                self.last_beacon = clock();
            }
            return Ok(None);
        }
        let Some(ap) = self.access_point else {
            return Ok(None);
        };
        if frame.len() >= wifi::MANAGEMENT_HEADER_BYTES + 2
            && matches!(frame[0], wifi::DISASSOCIATION | wifi::DEAUTHENTICATION)
            && frame[1] & wifi::DISTRIBUTION_DIRECTION_MASK == 0
            && frame[4..10] == self.identity.mac
            && frame[10..16] == ap.address
            && frame[16..22] == ap.address
        {
            self.disconnect(u16::from_le_bytes([frame[24], frame[25]]));
            return Ok(None);
        }
        if matches!(self.state, State::Authenticating | State::Associating) {
            let subtype = if self.state == State::Authenticating {
                wifi::AUTHENTICATION
            } else {
                wifi::ASSOCIATION_RESPONSE
            };
            if let Some(status) =
                wifi::management_reply_status(frame, subtype, self.identity.mac, ap.address)
            {
                if status != wifi::STATUS_SUCCESS {
                    self.disconnect(status);
                } else if self.state == State::Authenticating {
                    self.attempts = 0;
                    self.transition(State::Associating);
                    self.send_join()?;
                } else {
                    self.radio.configure_station_rates()?;
                    let nonce = random::nonce().ok_or(Error::Configuration)?;
                    self.supplicant = Some(wpa2::Supplicant::new(
                        *self.pmk,
                        self.identity.mac,
                        ap.address,
                        nonce,
                    ));
                    self.transition(State::Securing);
                }
            }
            return Ok(None);
        }
        if let Some(packet) = wifi::eapol_reply(frame, self.identity.mac, ap.address) {
            self.handle_eapol(packet, false)?;
            return Ok(None);
        }
        if !received.hardware_decrypted || self.state != State::Online {
            return Ok(None);
        }
        let mut ethernet = [0u8; MAX_ETHERNET_FRAME_LEN];
        let Some(length) = self
            .link
            .as_mut()
            .and_then(|link| link.receive(frame, &mut ethernet))
        else {
            return Ok(None);
        };
        self.last_beacon = clock();
        if ethernet[12..14] == EAPOL_ETHERTYPE {
            if ethernet[..6] == self.identity.mac && ethernet[6..12] == ap.address {
                self.handle_eapol(&ethernet[14..length], true)?;
            }
            return Ok(None);
        }
        Ok(Some(EthernetFrame::from_bytes(&ethernet[..length])))
    }
}
impl FrameDevice for WifiEthernet {
    fn mac_address(&self) -> [u8; 6] {
        self.identity.mac
    }
    fn transmit(&mut self, ethernet: &[u8]) -> bool {
        if !self.link_up() {
            return false;
        }
        match self.send_encrypted(ethernet) {
            Ok(()) => true,
            Err(error) => {
                self.hardware_failure(error);
                false
            }
        }
    }
    fn receive(&mut self) -> Option<EthernetFrame> {
        match self.poll_frame() {
            Ok(frame) => frame,
            Err(error) => {
                self.hardware_failure(error);
                None
            }
        }
    }
}
fn clock() -> u32 {
    unsafe { core::ptr::read_volatile(SYSTEM_TIMER_LOW) }
}
