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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Scanning,
    Authenticating,
    Associating,
    Securing,
    Online,
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
        if ssid.is_empty() || ssid.len() > 32 {
            return Err(Error::Configuration);
        }
        let mut radio = Rtl8188cu::attach(host, device)?;
        let identity = radio.identify()?;
        radio.power_off()?;
        radio.power_on()?;
        radio.initialize_packet_memory()?;
        radio.load_firmware(firmware)?;
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
            channel: 1,
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
            let mut frame = [0u8; 26];
            frame[0] = 0xc0;
            frame[4..10].copy_from_slice(&ap.address);
            frame[10..16].copy_from_slice(&self.identity.mac);
            frame[16..22].copy_from_slice(&ap.address);
            frame[24..26].copy_from_slice(&3u16.to_le_bytes());
            let _ = self.radio.transmit(&frame);
        }
        self.disconnect(3);
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
        self.disconnect(0);
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
            State::Scanning if elapsed >= 300_000 => {
                self.channel = self.channel % 13 + 1;
                self.radio.set_channel(self.channel)?;
                self.entered = now;
            }
            State::Authenticating | State::Associating if elapsed >= 1_000_000 => {
                if self.attempts >= 3 {
                    self.disconnect(0);
                } else {
                    self.send_join()?;
                }
            }
            State::Securing if elapsed >= 10_000_000 => self.disconnect(0),
            State::Online if now.wrapping_sub(self.last_beacon) >= 15_000_000 => self.disconnect(0),
            State::Backoff if elapsed >= 2_000_000 => {
                self.radio.clear_keys()?;
                self.channel = 1;
                self.radio.set_channel(1)?;
                self.transition(State::Scanning);
            }
            _ => {}
        }
        Ok(())
    }
    fn handle_eapol(&mut self, packet: &[u8], encrypted: bool) -> Result<(), Error> {
        let ap = self.access_point.ok_or(Error::Configuration)?.address;
        // Full pairwise reauthentication starts with a new station nonce.
        if self.state == State::Online && packet.get(5..7) == Some(&[0, 0x8a]) {
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
                self.radio.install_ccmp_key(4, 0, ap, &pairwise)?;
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
            ethernet[12..14].copy_from_slice(&[0x88, 0x8e]);
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
                if !(1..=13).contains(&channel) {
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
        if frame.len() >= 26
            && matches!(frame[0], 0xa0 | 0xc0)
            && frame[1] & 3 == 0
            && frame[4..10] == self.identity.mac
            && frame[10..16] == ap.address
            && frame[16..22] == ap.address
        {
            self.disconnect(u16::from_le_bytes([frame[24], frame[25]]));
            return Ok(None);
        }
        if matches!(self.state, State::Authenticating | State::Associating) {
            let subtype = if self.state == State::Authenticating {
                0xb0
            } else {
                0x10
            };
            if let Some(status) =
                wifi::management_reply_status(frame, subtype, self.identity.mac, ap.address)
            {
                if status != 0 {
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
        if ethernet[12..14] == [0x88, 0x8e] {
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
    unsafe { core::ptr::read_volatile(0x3f003004 as *const u32) }
}
