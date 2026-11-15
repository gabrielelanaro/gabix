//! IEEE 802.11 station packet parsing, shared by device adapters.
//!
//! The common 24-byte header carries frame control (0..2), duration (2..4),
//! three MAC addresses (4..10, 10..16, 16..22), and sequence/fragment (22..24).
//! Address roles depend on the To-DS/From-DS direction flags. A station's
//! management frames use receiver, sender, then access-point identity.
//! Multi-byte 802.11 fields are little-endian; embedded Ethernet protocol
//! identifiers are big-endian.

pub const MAX_NETWORK_NAME_BYTES: usize = 32;
pub const MANAGEMENT_HEADER_BYTES: usize = 24;
// First frame-control byte: type and subtype. The second byte holds flags.
pub const ASSOCIATION_REQUEST: u8 = 0x00;
pub const ASSOCIATION_RESPONSE: u8 = 0x10;
pub const BEACON: u8 = 0x80;
pub const DISASSOCIATION: u8 = 0xa0;
pub const AUTHENTICATION: u8 = 0xb0;
pub const DEAUTHENTICATION: u8 = 0xc0;
pub const DATA: u8 = 0x08;
pub const DISTRIBUTION_DIRECTION_MASK: u8 = 0b11;
// IEEE 802.11 reason 3: sending station is leaving the network.
pub const REASON_STATION_LEAVING: u16 = 3;
pub const STATUS_SUCCESS: u16 = 0;

// Address roles depend on frame direction; see the module header above.
pub const ADDRESS_1: core::ops::Range<usize> = 4..10;
pub const ADDRESS_2: core::ops::Range<usize> = 10..16;
pub const ADDRESS_3: core::ops::Range<usize> = 16..22;
const SEQUENCE_CONTROL: core::ops::Range<usize> = 22..24;
const SEQUENCE_SHIFT: u32 = 4; // Low four bits hold the fragment number.
const BEACON_ELEMENTS_OFFSET: usize = MANAGEMENT_HEADER_BYTES + 12;
const AUTHENTICATION_FRAME_BYTES: usize = MANAGEMENT_HEADER_BYTES + 6;
const ASSOCIATION_ELEMENTS_OFFSET: usize = MANAGEMENT_HEADER_BYTES + 4;
const ELEMENT_HEADER_BYTES: usize = 2; // Element ID, then byte length.
const ELEMENT_SSID: u8 = 0;
const ELEMENT_SUPPORTED_RATES: u8 = 1;
const ELEMENT_DS_PARAMETERS: u8 = 3;
const ELEMENT_RSN: u8 = 48;
const ELEMENT_EXTENDED_RATES: u8 = 50;
const CAPABILITY_ESS: u16 = 1;
const CAPABILITY_PRIVACY: u16 = 1 << 4;
const CAPABILITY_SHORT_PREAMBLE: u16 = 1 << 5;
const CAPABILITY_SHORT_SLOT: u16 = 1 << 10;
const LISTEN_INTERVAL_BEACONS: u16 = 10;
// Rates use 500 kbps units; bit 7 marks a basic (required) rate.
const SUPPORTED_RATES: [u8; 8] = [0x82, 0x84, 0x8b, 0x96, 12, 18, 24, 36];
const EXTENDED_RATES: [u8; 4] = [48, 72, 96, 108];
const OPEN_SYSTEM_REQUEST: [u8; 6] = [0, 0, 1, 0, 0, 0]; // Algorithm 0, transaction 1, status 0.
const OPEN_SYSTEM_REPLY_PREFIX: [u8; 4] = [0, 0, 2, 0]; // Algorithm 0, transaction 2.
const TO_DISTRIBUTION_SYSTEM: u8 = 1;
const FROM_DISTRIBUTION_SYSTEM: u8 = 2;
const PROTECTED_FRAME: u8 = 1 << 6;
// LLC/SNAP: AA AA, unnumbered information (03), RFC1042 OUI (00 00 00),
// followed by EtherType 0x888e for EAPOL.
const EAPOL_LLC_SNAP: [u8; 8] = [0xaa, 0xaa, 3, 0, 0, 0, 0x88, 0x8e];
const EAPOL_PAYLOAD_OFFSET: usize = MANAGEMENT_HEADER_BYTES + EAPOL_LLC_SNAP.len();

/// A beacon advertises an access point before a station has joined it.
pub struct Beacon<'a> {
    pub bssid: [u8; 6],
    pub ssid: &'a [u8],
    pub channel: Option<u8>,
    pub capabilities: u16,
    pub rsn: Option<&'a [u8]>,
}

pub fn parse_beacon(frame: &[u8]) -> Option<Beacon<'_>> {
    // 24-byte management header, 8-byte timestamp, interval and capabilities.
    if frame.len() < BEACON_ELEMENTS_OFFSET
        || frame[0] != BEACON
        || frame[1] & DISTRIBUTION_DIRECTION_MASK != 0
    {
        return None;
    }
    let mut bssid = [0; 6];
    bssid.copy_from_slice(&frame[ADDRESS_3]);
    let mut ssid = None;
    let mut channel = None;
    let mut rsn = None;
    let mut remaining = &frame[BEACON_ELEMENTS_OFFSET..];
    while !remaining.is_empty() {
        let header = remaining.get(..ELEMENT_HEADER_BYTES)?;
        let length = header[1] as usize;
        let value = remaining.get(ELEMENT_HEADER_BYTES..ELEMENT_HEADER_BYTES + length)?;
        match header[0] {
            ELEMENT_SSID if length <= MAX_NETWORK_NAME_BYTES && ssid.is_none() => {
                ssid = Some(value)
            }
            ELEMENT_SSID => return None,
            ELEMENT_DS_PARAMETERS if length == 1 => channel = Some(value[0]),
            ELEMENT_RSN if rsn.is_none() => rsn = Some(value),
            _ => {}
        }
        remaining = &remaining[ELEMENT_HEADER_BYTES + length..];
    }
    Some(Beacon {
        bssid,
        ssid: ssid?,
        channel,
        capabilities: u16::from_le_bytes([frame[34], frame[35]]),
        rsn,
    })
}

/// Build an open-system authentication request. WPA2 authenticates the shared
/// secret later, in EAPOL; this exchange establishes the 802.11 station first.
pub fn authentication_request(
    out: &mut [u8],
    station: [u8; 6],
    access_point: [u8; 6],
    sequence: u16,
) -> Option<usize> {
    let frame = out.get_mut(..AUTHENTICATION_FRAME_BYTES)?;
    management_header(frame, AUTHENTICATION, station, access_point, sequence);
    frame[MANAGEMENT_HEADER_BYTES..AUTHENTICATION_FRAME_BYTES]
        .copy_from_slice(&OPEN_SYSTEM_REQUEST);
    Some(frame.len())
}

/// Request a legacy 2.4 GHz station association with WPA2-PSK/CCMP.
/// This first station implementation advertises CCK/OFDM, without HT or QoS.
pub fn association_request(
    out: &mut [u8],
    station: [u8; 6],
    access_point: [u8; 6],
    sequence: u16,
    ssid: &[u8],
    access_point_capabilities: u16,
) -> Option<usize> {
    if ssid.len() > MAX_NETWORK_NAME_BYTES {
        return None;
    }
    // RSN v1: one CCMP group cipher, one CCMP pairwise cipher and one PSK
    // authentication suite; capabilities zero. Suite OUI is 00:0f:ac.
    const RSN: [u8; 20] = [
        1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];
    let length = ASSOCIATION_ELEMENTS_OFFSET
        + 4 * ELEMENT_HEADER_BYTES
        + ssid.len()
        + SUPPORTED_RATES.len()
        + EXTENDED_RATES.len()
        + RSN.len();
    let frame = out.get_mut(..length)?;
    management_header(frame, ASSOCIATION_REQUEST, station, access_point, sequence);
    // ESS/privacy plus supported short-preamble and short-slot capabilities.
    let capabilities = CAPABILITY_ESS
        | CAPABILITY_PRIVACY
        | (access_point_capabilities & (CAPABILITY_SHORT_PREAMBLE | CAPABILITY_SHORT_SLOT));
    frame[24..26].copy_from_slice(&capabilities.to_le_bytes());
    frame[26..28].copy_from_slice(&LISTEN_INTERVAL_BEACONS.to_le_bytes());
    let mut cursor = ASSOCIATION_ELEMENTS_OFFSET;
    for (id, value) in [
        (ELEMENT_SSID, ssid),
        (ELEMENT_SUPPORTED_RATES, &SUPPORTED_RATES[..]),
        (ELEMENT_EXTENDED_RATES, &EXTENDED_RATES[..]),
        (ELEMENT_RSN, &RSN[..]),
    ] {
        frame[cursor] = id;
        frame[cursor + 1] = value.len() as u8;
        frame[cursor + ELEMENT_HEADER_BYTES..cursor + ELEMENT_HEADER_BYTES + value.len()]
            .copy_from_slice(value);
        cursor += ELEMENT_HEADER_BYTES + value.len();
    }
    Some(length)
}

fn management_header(
    frame: &mut [u8],
    subtype: u8,
    station: [u8; 6],
    access_point: [u8; 6],
    sequence: u16,
) {
    frame.fill(0);
    frame[0] = subtype;
    frame[ADDRESS_1].copy_from_slice(&access_point);
    frame[ADDRESS_2].copy_from_slice(&station);
    frame[ADDRESS_3].copy_from_slice(&access_point);
    frame[SEQUENCE_CONTROL].copy_from_slice(&(sequence << SEQUENCE_SHIFT).to_le_bytes());
}

/// Validate that a management reply belongs to this station and access point.
/// The caller supplies AUTHENTICATION or ASSOCIATION_RESPONSE.
pub fn management_reply_status(
    frame: &[u8],
    subtype: u8,
    station: [u8; 6],
    access_point: [u8; 6],
) -> Option<u16> {
    if frame.len() < AUTHENTICATION_FRAME_BYTES
        || frame[0] != subtype
        || frame[1] & DISTRIBUTION_DIRECTION_MASK != 0
        || frame[ADDRESS_1] != station
        || frame[ADDRESS_2] != access_point
        || frame[ADDRESS_3] != access_point
    {
        return None;
    }
    let offset = match subtype {
        AUTHENTICATION if frame[24..28] == OPEN_SYSTEM_REPLY_PREFIX => 28,
        ASSOCIATION_RESPONSE => 26,
        _ => return None,
    };
    Some(u16::from_le_bytes([frame[offset], frame[offset + 1]]))
}

/// Encapsulate EAPOL in a station-to-access-point data frame.
pub fn eapol_request(
    out: &mut [u8],
    station: [u8; 6],
    ap: [u8; 6],
    sequence: u16,
    eapol: &[u8],
) -> Option<usize> {
    let length = EAPOL_PAYLOAD_OFFSET + eapol.len();
    let frame = out.get_mut(..length)?;
    management_header(frame, DATA, station, ap, sequence);
    frame[1] = TO_DISTRIBUTION_SYSTEM; // Receiver AP in address 1; destination in address 3.
    frame[MANAGEMENT_HEADER_BYTES..EAPOL_PAYLOAD_OFFSET].copy_from_slice(&EAPOL_LLC_SNAP);
    frame[EAPOL_PAYLOAD_OFFSET..].copy_from_slice(eapol);
    Some(length)
}

/// Extract an unencrypted EAPOL frame sent by the selected access point.
pub fn eapol_reply<'a>(frame: &'a [u8], station: [u8; 6], ap: [u8; 6]) -> Option<&'a [u8]> {
    if frame.len() < EAPOL_PAYLOAD_OFFSET
        || frame[0] != DATA
        || frame[1] & (PROTECTED_FRAME | DISTRIBUTION_DIRECTION_MASK) != FROM_DISTRIBUTION_SYSTEM
        || frame[ADDRESS_1] != station
        || frame[ADDRESS_2] != ap
    {
        return None;
    }
    if frame[MANAGEMENT_HEADER_BYTES..EAPOL_PAYLOAD_OFFSET] != EAPOL_LLC_SNAP {
        return None;
    }
    Some(&frame[EAPOL_PAYLOAD_OFFSET..])
}

/// Ethernet encapsulation for an associated CCMP station. The adapter performs
/// AES encryption/decryption; this object owns packet numbers and rejects replay.
pub struct CcmpLink {
    station: [u8; 6],
    ap: [u8; 6],
    sequence: u16,
    transmit_number: u64,
    pairwise_receive: [u64; 16],
    group_receive: [[u64; 16]; 4],
    group_installed: [bool; 4],
}
impl CcmpLink {
    pub fn new(
        station: [u8; 6],
        ap: [u8; 6],
        group_id: u8,
        group_counter: [u8; 8],
    ) -> Option<Self> {
        if group_id > 3 {
            return None;
        }
        let mut link = Self {
            station,
            ap,
            sequence: 0,
            transmit_number: 0,
            pairwise_receive: [0; 16],
            group_receive: [[0; 16]; 4],
            group_installed: [false; 4],
        };
        link.install_group(group_id, group_counter)?;
        Some(link)
    }

    /// Call only for a newly authenticated GTK, never for its retransmission.
    pub fn install_group(&mut self, group_id: u8, counter: [u8; 8]) -> Option<()> {
        if group_id > 3 || counter[6..] != [0, 0] {
            return None;
        }
        self.group_installed[group_id as usize] = true;
        self.group_receive[group_id as usize].fill(u64::from_le_bytes(counter));
        Some(())
    }

    pub fn transmit(&mut self, ethernet: &[u8], out: &mut [u8]) -> Option<usize> {
        if ethernet.len() < 14
            || ethernet[6..12] != self.station
            || self.transmit_number == 0xffffffffffff
        {
            return None;
        }
        let length = 40 + ethernet.len() - 14;
        let frame = out.get_mut(..length)?;
        management_header(frame, DATA, self.station, self.ap, self.sequence);
        self.sequence = self.sequence.wrapping_add(1) & 0xfff;
        frame[1] = 0x41; // To DS, protected by CCMP.
        frame[16..22].copy_from_slice(&ethernet[..6]);
        self.transmit_number += 1;
        let pn = self.transmit_number.to_le_bytes();
        frame[24..32].copy_from_slice(&[pn[0], pn[1], 0, 0x20, pn[2], pn[3], pn[4], pn[5]]);
        frame[32..38].copy_from_slice(&[0xaa, 0xaa, 3, 0, 0, 0]);
        frame[38..40].copy_from_slice(&ethernet[12..14]);
        frame[40..].copy_from_slice(&ethernet[14..]);
        Some(length)
    }

    /// Call only after the adapter reports successful CCMP decryption and MIC
    /// verification. The hardware retains the 8-byte CCMP header and MIC.
    pub fn receive(&mut self, frame: &[u8], out: &mut [u8]) -> Option<usize> {
        if frame.len() < 48
            || !matches!(frame[0], 0x08 | 0x88)
            || frame[1] & 0xc7 != 0x42
            || frame[10..16] != self.ap
            || frame[22] & 15 != 0
        {
            return None;
        }
        let group = frame[4] & 1 != 0;
        if !group && frame[4..10] != self.station {
            return None;
        }
        let (header, tid) = if frame[0] == 0x88 {
            // A-MSDU carries multiple Ethernet frames and needs its own parser.
            if frame[24] & 0x80 != 0 {
                return None;
            }
            (26, (frame[24] & 15) as usize)
        } else {
            (24, 0)
        };
        let iv = frame.get(header..header + 8)?;
        if iv[2] != 0 || iv[3] & 0x3f != 0x20 {
            return None;
        }
        let key_id = (iv[3] >> 6) as usize;
        let counter = u64::from_le_bytes([iv[0], iv[1], iv[4], iv[5], iv[6], iv[7], 0, 0]);
        let previous = if group {
            if !self.group_installed[key_id] {
                return None;
            }
            &mut self.group_receive[key_id][tid]
        } else {
            if key_id != 0 {
                return None;
            }
            &mut self.pairwise_receive[tid]
        };
        if counter <= *previous {
            return None;
        }
        let payload = frame.get(header + 8..frame.len().checked_sub(8)?)?;
        if payload.len() < 8 || payload[..6] != [0xaa, 0xaa, 3, 0, 0, 0] {
            return None;
        }
        let length = 14 + payload.len() - 8;
        let ethernet = out.get_mut(..length)?;
        ethernet[..6].copy_from_slice(&frame[4..10]);
        ethernet[6..12].copy_from_slice(&frame[16..22]);
        ethernet[12..14].copy_from_slice(&payload[6..8]);
        ethernet[14..].copy_from_slice(&payload[8..]);
        *previous = counter;
        Some(length)
    }
}
