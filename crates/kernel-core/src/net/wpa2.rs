//! WPA2-Personal CCMP four-way handshake (RSN descriptor version 2).
//! The caller supplies a private PMK and a fresh hardware-random station nonce.
//! Frame transport and adapter key installation remain outside this module.
use aes::{
    cipher::{Array, BlockCipherDecrypt, KeyInit as CipherKeyInit},
    Aes128,
};
use hmac::{Hmac, Mac};
use sha1::Sha1;

pub const RSN: [u8; 22] = [
    48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
];
const MIC: core::ops::Range<usize> = 81..97;
// Byte ranges start at the EAPOL version byte. The fixed key header is
// followed by key data; its two-byte length excludes the fixed header.
const EAPOL_HEADER_BYTES: usize = 4;
const KEY_DATA_OFFSET: usize = 99;
const EAPOL_KEY_PACKET: u8 = 3;
const RSN_KEY_DESCRIPTOR: u8 = 2;
const BODY_LENGTH: core::ops::Range<usize> = 2..4;
const KEY_INFORMATION: core::ops::Range<usize> = 5..7;
const KEY_LENGTH: core::ops::Range<usize> = 7..9;
const REPLAY_COUNTER: core::ops::Range<usize> = 9..17;
const NONCE: core::ops::Range<usize> = 17..49;
const GROUP_RECEIVE_COUNTER: core::ops::Range<usize> = 65..73;
const KEY_DATA_LENGTH: core::ops::Range<usize> = 97..99;

// Exact accepted key-information words. Matching the whole word retains our
// restriction to WPA2/CCMP and rejects unsupported combinations of flags.
const PAIRWISE_KEY: u16 = 1 << 3;
const PAIRWISE_MESSAGE_1: u16 = 0x008a;
const PAIRWISE_MESSAGE_2: u16 = 0x010a;
const PAIRWISE_MESSAGE_3: u16 = 0x13ca;
const PAIRWISE_MESSAGE_4: u16 = 0x030a;
const GROUP_MESSAGE_1: u16 = 0x1382;
const GROUP_MESSAGE_1_WITH_TX_RX: u16 = 0x13c2;
const GROUP_MESSAGE_2: u16 = 0x0302;

// The derived pairwise transient key contains three separate 16-byte keys:
// authenticate handshake messages, unwrap key data, encrypt unicast traffic.
const MESSAGE_AUTHENTICATION_KEY: core::ops::Range<usize> = 0..16;
const KEY_ENCRYPTION_KEY: core::ops::Range<usize> = 16..32;
const UNICAST_ENCRYPTION_KEY: core::ops::Range<usize> = 32..48;

// Key-data elements: ID, body length, then a four-byte RSN selector.
// Selectors follow hostap's RSN_KEY_DATA_* definitions in wpa_common.h.
const ELEMENT_HEADER_BYTES: usize = 2;
const RSN_ELEMENT: u8 = 48;
const VENDOR_ELEMENT: u8 = 0xdd;
const KEY_DATA_SELECTOR: core::ops::Range<usize> = 2..6;
const PMKID_SELECTOR: [u8; 4] = [0, 15, 172, 4];
const GROUP_KEY_SELECTOR: [u8; 4] = [0, 15, 172, 1];
const PMKID_BODY_BYTES: usize = 20;
const PMKID: core::ops::Range<usize> = 6..22;
const GROUP_KEY_BODY_BYTES: usize = 22;
const GROUP_KEY_FLAGS: usize = 6;
const GROUP_KEY_ID_MASK: u8 = 0b11;
const GROUP_KEY_ALLOWED_FLAGS: u8 = 0b111;
const GROUP_KEY_BYTES: core::ops::Range<usize> = 8..24;

// RFC 3394 uses 64-bit data units and six rounds of 128-bit AES operations.
const WRAP_UNIT_BYTES: usize = 8;
const WRAP_ROUNDS: usize = 6;
const MIN_WRAPPED_BYTES: usize = 3 * WRAP_UNIT_BYTES;
const WRAP_INTEGRITY_VALUE: [u8; WRAP_UNIT_BYTES] = [0xa6; WRAP_UNIT_BYTES];

#[derive(Clone, Copy)]
enum ReplyNonce {
    Station,
    Zero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Malformed,
    Unsupported,
    Replay,
    Nonce,
    Mic,
    KeyData,
    Capacity,
    PmkMismatch,
}

/// These bytes are secret. Deliberately omit Debug and never put them in logs.
pub struct InstalledKeys {
    pub pairwise: Option<[u8; 16]>,
    pub group: [u8; 16],
    pub group_id: u8,
    pub group_receive_counter: [u8; 8],
}
pub struct Reply {
    pub length: usize,
    pub install: Option<InstalledKeys>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    AwaitMessage1,
    AwaitMessage3,
    Established,
}

pub struct Supplicant {
    pmk: [u8; 32],
    pub pmk_confirmed: bool,
    station: [u8; 6],
    access_point: [u8; 6],
    station_nonce: [u8; 32],
    ap_nonce: [u8; 32],
    ptk: [u8; 48],
    replay: u64,
    message3_mic: [u8; 16],
    group_mic: Option<[u8; 16]>,
    current_group: Option<(u8, [u8; 16])>,
    state: State,
}

impl Supplicant {
    pub fn new(
        pmk: [u8; 32],
        station: [u8; 6],
        access_point: [u8; 6],
        station_nonce: [u8; 32],
    ) -> Self {
        Self {
            pmk,
            pmk_confirmed: false,
            station,
            access_point,
            station_nonce,
            ap_nonce: [0; 32],
            ptk: [0; 48],
            replay: 0,
            message3_mic: [0; 16],
            group_mic: None,
            current_group: None,
            state: State::AwaitMessage1,
        }
    }
    pub fn established(&self) -> bool {
        self.state == State::Established
    }

    /// Process an EAPOL packet after the caller checks its source BSSID.
    /// A duplicate message 3 gets another message 4, without reinstalling keys.
    pub fn receive(&mut self, packet: &[u8], out: &mut [u8]) -> Result<Reply, Error> {
        if packet.len() < KEY_DATA_OFFSET
            || packet[1] != EAPOL_KEY_PACKET
            || packet[4] != RSN_KEY_DESCRIPTOR
            || !matches!(packet[0], 1 | 2)
        {
            return Err(Error::Malformed);
        }
        let length = EAPOL_HEADER_BYTES
            + u16::from_be_bytes(packet[BODY_LENGTH].try_into().unwrap()) as usize;
        let packet = packet.get(..length).ok_or(Error::Malformed)?;
        if packet.len() < KEY_DATA_OFFSET {
            return Err(Error::Malformed);
        }
        let data_length = u16::from_be_bytes(packet[KEY_DATA_LENGTH].try_into().unwrap()) as usize;
        if KEY_DATA_OFFSET + data_length != packet.len() {
            return Err(Error::Malformed);
        }
        let flags = u16::from_be_bytes(packet[KEY_INFORMATION].try_into().unwrap());
        let key_length = u16::from_be_bytes(packet[KEY_LENGTH].try_into().unwrap());
        let group_key = flags & PAIRWISE_KEY == 0;
        if key_length != 16 && !(group_key && key_length == 0) {
            return Err(Error::Unsupported);
        }
        let replay = u64::from_be_bytes(packet[REPLAY_COUNTER].try_into().unwrap());
        match flags {
            PAIRWISE_MESSAGE_1 => self.message1(packet, replay, out),
            PAIRWISE_MESSAGE_3 => self.message3(packet, replay, out),
            GROUP_MESSAGE_1 | GROUP_MESSAGE_1_WITH_TX_RX => {
                self.group_message1(packet, replay, out)
            }
            _ => Err(Error::Unsupported),
        }
    }

    fn message1(&mut self, packet: &[u8], replay: u64, out: &mut [u8]) -> Result<Reply, Error> {
        if self.state == State::Established {
            return Err(Error::Replay);
        }
        // Some access points include PMKID in message 1. It lets us verify
        // the configured PMK before debugging the reply's radio delivery.
        let mut data = &packet[KEY_DATA_OFFSET..];
        while !data.is_empty() {
            if data.len() < ELEMENT_HEADER_BYTES {
                return Err(Error::KeyData);
            }
            let length = data[1] as usize;
            let ie = data
                .get(..ELEMENT_HEADER_BYTES + length)
                .ok_or(Error::KeyData)?;
            if ie[0] == VENDOR_ELEMENT && ie.get(KEY_DATA_SELECTOR) == Some(&PMKID_SELECTOR) {
                if length != PMKID_BODY_BYTES {
                    return Err(Error::KeyData);
                }
                let mut mac = <Hmac<Sha1> as hmac::KeyInit>::new_from_slice(&self.pmk).unwrap();
                mac.update(b"PMK Name");
                mac.update(&self.access_point);
                mac.update(&self.station);
                mac.verify_truncated_left(&ie[PMKID])
                    .map_err(|_| Error::PmkMismatch)?;
                self.pmk_confirmed = true;
            }
            data = &data[ELEMENT_HEADER_BYTES + length..];
        }
        if packet[NONCE].iter().all(|b| *b == 0) {
            return Err(Error::Nonce);
        }
        if self.state == State::AwaitMessage3 {
            if replay < self.replay {
                return Err(Error::Replay);
            }
            if packet[NONCE] != self.ap_nonce {
                return Err(Error::Nonce);
            }
        } else {
            self.ap_nonce.copy_from_slice(&packet[NONCE]);
            self.ptk = derive_ptk(
                &self.pmk,
                self.station,
                self.access_point,
                self.station_nonce,
                self.ap_nonce,
            );
        }
        let length = self.reply(
            out,
            packet[0],
            PAIRWISE_MESSAGE_2,
            replay,
            ReplyNonce::Station,
            &RSN,
        )?;
        self.replay = replay;
        self.state = State::AwaitMessage3;
        Ok(Reply {
            length,
            install: None,
        })
    }

    fn message3(&mut self, packet: &[u8], replay: u64, out: &mut [u8]) -> Result<Reply, Error> {
        if self.state == State::AwaitMessage1 {
            return Err(Error::Replay);
        }
        if packet[NONCE] != self.ap_nonce {
            return Err(Error::Nonce);
        }
        verify_mic(&self.ptk[MESSAGE_AUTHENTICATION_KEY], packet)?;
        if self.state == State::Established {
            if replay != self.replay || packet[MIC] != self.message3_mic {
                return Err(Error::Replay);
            }
            return Ok(Reply {
                length: self.reply(
                    out,
                    packet[0],
                    PAIRWISE_MESSAGE_4,
                    replay,
                    ReplyNonce::Zero,
                    &[],
                )?,
                install: None,
            });
        }
        if replay <= self.replay {
            return Err(Error::Replay);
        }
        let mut unwrapped = [0u8; 512];
        let length = unwrap_key(
            &self.ptk[KEY_ENCRYPTION_KEY],
            &packet[KEY_DATA_OFFSET..],
            &mut unwrapped,
        )?;
        let (group_id, group) = parse_key_data(&unwrapped[..length], true)?;
        let keys = InstalledKeys {
            pairwise: Some(self.ptk[UNICAST_ENCRYPTION_KEY].try_into().unwrap()),
            group,
            group_id,
            group_receive_counter: packet[GROUP_RECEIVE_COUNTER].try_into().unwrap(),
        };
        let length = self.reply(
            out,
            packet[0],
            PAIRWISE_MESSAGE_4,
            replay,
            ReplyNonce::Zero,
            &[],
        )?;
        self.message3_mic.copy_from_slice(&packet[MIC]);
        self.current_group = Some((group_id, group));
        self.replay = replay;
        self.state = State::Established;
        Ok(Reply {
            length,
            install: Some(keys),
        })
    }

    /// Rotate the broadcast/multicast key while retaining the pairwise key.
    /// Retransmissions get an acknowledgement without resetting receive counters.
    fn group_message1(
        &mut self,
        packet: &[u8],
        replay: u64,
        out: &mut [u8],
    ) -> Result<Reply, Error> {
        if self.state != State::Established || replay < self.replay {
            return Err(Error::Replay);
        }
        verify_mic(&self.ptk[MESSAGE_AUTHENTICATION_KEY], packet)?;
        if replay == self.replay {
            if self.group_mic.as_ref().map(|mic| &mic[..]) != Some(&packet[MIC]) {
                return Err(Error::Replay);
            }
            return Ok(Reply {
                length: self.reply(
                    out,
                    packet[0],
                    GROUP_MESSAGE_2,
                    replay,
                    ReplyNonce::Zero,
                    &[],
                )?,
                install: None,
            });
        }
        let mut unwrapped = [0u8; 512];
        let length = unwrap_key(
            &self.ptk[KEY_ENCRYPTION_KEY],
            &packet[KEY_DATA_OFFSET..],
            &mut unwrapped,
        )?;
        let (group_id, group) = parse_key_data(&unwrapped[..length], false)?;
        // A repeated key with a fresh replay counter must preserve its PN state.
        let install = match self.current_group {
            Some((old_id, old_key)) if old_key == group => {
                if old_id != group_id {
                    return Err(Error::KeyData);
                }
                None
            }
            _ => Some(InstalledKeys {
                pairwise: None,
                group,
                group_id,
                group_receive_counter: packet[GROUP_RECEIVE_COUNTER].try_into().unwrap(),
            }),
        };
        let length = self.reply(
            out,
            packet[0],
            GROUP_MESSAGE_2,
            replay,
            ReplyNonce::Zero,
            &[],
        )?;
        self.current_group = Some((group_id, group));
        self.group_mic = Some(packet[MIC].try_into().unwrap());
        self.replay = replay;
        Ok(Reply { length, install })
    }

    fn reply(
        &self,
        out: &mut [u8],
        version: u8,
        flags: u16,
        replay: u64,
        nonce: ReplyNonce,
        data: &[u8],
    ) -> Result<usize, Error> {
        let length = KEY_DATA_OFFSET + data.len();
        let out = out.get_mut(..length).ok_or(Error::Capacity)?;
        // Message 2 sends our nonce; later acknowledgements keep this field zero.
        out.fill(0);
        out[0] = version;
        out[1] = EAPOL_KEY_PACKET;
        out[4] = RSN_KEY_DESCRIPTOR;
        out[BODY_LENGTH].copy_from_slice(&((length - EAPOL_HEADER_BYTES) as u16).to_be_bytes());
        out[KEY_INFORMATION].copy_from_slice(&flags.to_be_bytes());
        out[REPLAY_COUNTER].copy_from_slice(&replay.to_be_bytes());
        if matches!(nonce, ReplyNonce::Station) {
            out[NONCE].copy_from_slice(&self.station_nonce);
        }
        out[KEY_DATA_LENGTH].copy_from_slice(&(data.len() as u16).to_be_bytes());
        out[KEY_DATA_OFFSET..].copy_from_slice(data);
        let mic = hmac_sha1(&self.ptk[MESSAGE_AUTHENTICATION_KEY], &[out]);
        out[MIC].copy_from_slice(&mic[..16]);
        Ok(length)
    }
}

fn hmac_sha1(key: &[u8], parts: &[&[u8]]) -> [u8; 20] {
    let mut mac = <Hmac<Sha1> as hmac::KeyInit>::new_from_slice(key).unwrap();
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}
fn derive_ptk(
    pmk: &[u8; 32],
    station: [u8; 6],
    ap: [u8; 6],
    snonce: [u8; 32],
    anonce: [u8; 32],
) -> [u8; 48] {
    let (first_mac, second_mac) = if station < ap {
        (station, ap)
    } else {
        (ap, station)
    };
    let (first_nonce, second_nonce) = if snonce < anonce {
        (snonce, anonce)
    } else {
        (anonce, snonce)
    };
    let mut ptk = [0; 48];
    for (counter, chunk) in ptk.chunks_mut(20).enumerate() {
        let digest = hmac_sha1(
            pmk,
            &[
                b"Pairwise key expansion\0",
                &first_mac,
                &second_mac,
                &first_nonce,
                &second_nonce,
                &[counter as u8],
            ],
        );
        chunk.copy_from_slice(&digest[..chunk.len()]);
    }
    ptk
}
fn verify_mic(key: &[u8], packet: &[u8]) -> Result<(), Error> {
    let mut mac = <Hmac<Sha1> as hmac::KeyInit>::new_from_slice(key).unwrap();
    // Compute authentication with the packet's MIC field treated as zero.
    mac.update(&packet[..MIC.start]);
    mac.update(&[0; 16]);
    mac.update(&packet[MIC.end..]);
    mac.verify_truncated_left(&packet[MIC])
        .map_err(|_| Error::Mic)
}

/// RFC 3394 section 2.2.2; AES primitive comes from RustCrypto.
fn unwrap_key(key: &[u8], wrapped: &[u8], out: &mut [u8]) -> Result<usize, Error> {
    // One integrity unit precedes at least two data units.
    if wrapped.len() < MIN_WRAPPED_BYTES
        || wrapped.len() % WRAP_UNIT_BYTES != 0
        || wrapped.len() - WRAP_UNIT_BYTES > out.len()
    {
        return Err(Error::KeyData);
    }
    let cipher = Aes128::new_from_slice(key).map_err(|_| Error::KeyData)?;
    let length = wrapped.len() - WRAP_UNIT_BYTES;
    let data_units = length / WRAP_UNIT_BYTES;
    // RFC 3394 calls this evolving integrity value A, and the output units R[i].
    let mut integrity: [u8; WRAP_UNIT_BYTES] = wrapped[..WRAP_UNIT_BYTES].try_into().unwrap();
    out[..length].copy_from_slice(&wrapped[WRAP_UNIT_BYTES..]);
    for round in (0..WRAP_ROUNDS).rev() {
        for unit in (1..=data_units).rev() {
            let step = (data_units * round + unit) as u64;
            let unit_range = (unit - 1) * WRAP_UNIT_BYTES..unit * WRAP_UNIT_BYTES;
            // Undo one wrap step: decrypt (A XOR step) followed by R[unit].
            let mut block = [0u8; 16];
            block[..WRAP_UNIT_BYTES]
                .copy_from_slice(&(u64::from_be_bytes(integrity) ^ step).to_be_bytes());
            block[WRAP_UNIT_BYTES..].copy_from_slice(&out[unit_range.clone()]);
            let mut block = Array::from(block);
            cipher.decrypt_block(&mut block);
            integrity.copy_from_slice(&block[..WRAP_UNIT_BYTES]);
            out[unit_range].copy_from_slice(&block[WRAP_UNIT_BYTES..]);
        }
    }
    // Only release the recovered data when the integrity value matches.
    if integrity != WRAP_INTEGRITY_VALUE {
        out[..length].fill(0);
        return Err(Error::KeyData);
    }
    Ok(length)
}
fn parse_key_data(mut bytes: &[u8], require_rsn: bool) -> Result<(u8, [u8; 16]), Error> {
    let mut rsn = false;
    let mut gtk = None;
    while !bytes.is_empty() {
        // IEEE key-data padding begins with 0xdd followed by zero bytes.
        if bytes[0] == VENDOR_ELEMENT && bytes[1..].iter().all(|b| *b == 0) {
            break;
        }
        if bytes.len() < ELEMENT_HEADER_BYTES {
            return Err(Error::KeyData);
        }
        let length = bytes[1] as usize;
        let ie = bytes
            .get(..ELEMENT_HEADER_BYTES + length)
            .ok_or(Error::KeyData)?;
        if ie[0] == RSN_ELEMENT {
            if rsn || ie != RSN {
                return Err(Error::Unsupported);
            }
            rsn = true;
        } else if ie[0] == VENDOR_ELEMENT && ie.get(KEY_DATA_SELECTOR) == Some(&GROUP_KEY_SELECTOR)
        {
            if gtk.is_some()
                || length != GROUP_KEY_BODY_BYTES
                || ie[GROUP_KEY_FLAGS] & !GROUP_KEY_ALLOWED_FLAGS != 0
            {
                return Err(Error::KeyData);
            }
            gtk = Some((
                ie[GROUP_KEY_FLAGS] & GROUP_KEY_ID_MASK,
                ie[GROUP_KEY_BYTES].try_into().unwrap(),
            ));
        }
        bytes = &bytes[ELEMENT_HEADER_BYTES + length..];
    }
    if require_rsn && !rsn {
        return Err(Error::KeyData);
    }
    gtk.ok_or(Error::KeyData)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hex<const N: usize>(s: &str) -> [u8; N] {
        assert_eq!(s.len(), N * 2);
        let mut out = [0; N];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }
    #[test]
    fn test_handshake_authenticates_without_reinstalling_keys() {
        // Public synthetic vector, independently generated with Python hmac
        // and cryptography's RFC3394 implementation. Contains no user keys.
        let mut client = Supplicant::new(
            core::array::from_fn(|i| i as u8),
            [2, 0, 0, 0, 0, 1],
            [2, 0, 0, 0, 0, 2],
            core::array::from_fn(|i| i as u8 + 32),
        );
        let m1 = hex::<99>("0203005f02008a00100000000000000001404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
        let m3 = hex::<155>("020300970213ca00100000000000000002404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f00000000000000000000000000000000000000000000000000000000000000005a60674d8f6d99f2d1d748117db2689f0038709c7bc1ff6e165a5caafa066d950f11da96d2b41f91a8d1ff6000aa18e2b2f3eba7b4d8f11adf83cb9678f4129a95a8df35f8cd8ca38f23");
        let m2 = hex::<121>("0203007502010a00000000000000000001202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f00000000000000000000000000000000000000000000000000000000000000009551692251cfe260fb968488e0df2ec6001630140100000fac040100000fac040100000fac020000");
        let m4 = hex::<99>("0203005f02030a0000000000000000000200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000add21559b80b4a94dfe0feeb3fac436b0000");
        let mut out = [0u8; 256];
        let reply = client.receive(&m1, &mut out).unwrap();
        assert!(reply.install.is_none());
        assert_eq!(&out[..reply.length], &m2);
        let mut corrupted = m3;
        corrupted[90] ^= 1;
        assert!(matches!(
            client.receive(&corrupted, &mut out),
            Err(Error::Mic)
        ));
        assert!(!client.established());
        let reply = client.receive(&m3, &mut out).unwrap();
        let keys = reply.install.unwrap();
        assert_eq!(
            keys.pairwise,
            Some(hex::<16>("8ec6663d093eb76401bdfc7763646499"))
        );
        assert_eq!(keys.group, core::array::from_fn(|i| i as u8 + 160));
        assert_eq!(keys.group_id, 1);
        assert_eq!(&out[..reply.length], &m4);
        assert!(client.established());
        let duplicate = client.receive(&m3, &mut out).unwrap();
        assert!(duplicate.install.is_none());
        assert_eq!(&out[..duplicate.length], &m4);
        assert!(matches!(client.receive(&m1, &mut out), Err(Error::Replay)));
        let group1 = hex::<131>("0203007f0213820000000000000000000300000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000465876032ca4699f354037223fef64ae0020250d29669d15eeb7d8de1dee2864d3dfc7a77e8850f668deb3ac88398f7b6562");
        let group2 = hex::<99>("0203005f02030200000000000000000003000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008a73b22350e2a9504aa74fccc7a984b00000");
        let reply = client.receive(&group1, &mut out).unwrap();
        let keys = reply.install.unwrap();
        assert!(keys.pairwise.is_none());
        assert_eq!(keys.group_id, 2);
        assert_eq!(keys.group, core::array::from_fn(|i| i as u8 + 176));
        assert_eq!(&out[..reply.length], &group2);
        assert!(client.receive(&group1, &mut out).unwrap().install.is_none());
        assert!(matches!(client.receive(&m3, &mut out), Err(Error::Replay)));
    }
    #[test]
    fn test_unwrap_accepts_rfc3394_vector_and_rejects_corruption() {
        let key = hex::<16>("000102030405060708090a0b0c0d0e0f");
        let mut wrapped = hex::<24>("1fa68b0a8112b447aef34bd8fb5a7b829d3e862371d2cfe5");
        let mut out = [0; 16];
        assert_eq!(unwrap_key(&key, &wrapped, &mut out), Ok(16));
        assert_eq!(out, hex::<16>("00112233445566778899aabbccddeeff"));
        wrapped[2] ^= 1;
        assert_eq!(unwrap_key(&key, &wrapped, &mut out), Err(Error::KeyData));
    }
}
