//! IEEE 802.11 station packet parsing, shared by device adapters.

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
    if frame.len() < 36 || frame[0] != 0x80 || frame[1] & 3 != 0 {
        return None;
    }
    let mut bssid = [0; 6];
    bssid.copy_from_slice(&frame[16..22]);
    let mut ssid = None;
    let mut channel = None;
    let mut rsn = None;
    let mut remaining = &frame[36..];
    while !remaining.is_empty() {
        let header = remaining.get(..2)?;
        let length = header[1] as usize;
        let value = remaining.get(2..2 + length)?;
        match header[0] {
            0 if length <= 32 && ssid.is_none() => ssid = Some(value),
            0 => return None,
            3 if length == 1 => channel = Some(value[0]),
            48 if rsn.is_none() => rsn = Some(value),
            _ => {}
        }
        remaining = &remaining[2 + length..];
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
    let frame = out.get_mut(..30)?;
    management_header(frame, 0xb0, station, access_point, sequence);
    frame[24..30].copy_from_slice(&[0, 0, 1, 0, 0, 0]);
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
    if ssid.len() > 32 {
        return None;
    }
    const RSN: [u8; 20] = [
        1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
    ];
    let length = 28 + 2 + ssid.len() + 10 + 6 + 22;
    let frame = out.get_mut(..length)?;
    management_header(frame, 0x00, station, access_point, sequence);
    // ESS/privacy plus supported short-preamble and short-slot capabilities.
    let capabilities = 0x0011 | (access_point_capabilities & 0x0420);
    frame[24..26].copy_from_slice(&capabilities.to_le_bytes());
    frame[26..28].copy_from_slice(&10u16.to_le_bytes());
    let mut cursor = 28;
    for (id, value) in [
        (0, ssid),
        (1, &[0x82, 0x84, 0x8b, 0x96, 12, 18, 24, 36][..]),
        (50, &[48, 72, 96, 108][..]),
        (48, &RSN[..]),
    ] {
        frame[cursor] = id;
        frame[cursor + 1] = value.len() as u8;
        frame[cursor + 2..cursor + 2 + value.len()].copy_from_slice(value);
        cursor += 2 + value.len();
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
    frame[4..10].copy_from_slice(&access_point);
    frame[10..16].copy_from_slice(&station);
    frame[16..22].copy_from_slice(&access_point);
    frame[22..24].copy_from_slice(&(sequence << 4).to_le_bytes());
}

/// Validate that a management reply belongs to this station and access point.
/// The caller supplies 0xb0 for authentication or 0x10 for association.
pub fn management_reply_status(
    frame: &[u8],
    subtype: u8,
    station: [u8; 6],
    access_point: [u8; 6],
) -> Option<u16> {
    if frame.len() < 30
        || frame[0] != subtype
        || frame[1] & 3 != 0
        || frame[4..10] != station
        || frame[10..16] != access_point
        || frame[16..22] != access_point
    {
        return None;
    }
    let offset = match subtype {
        0xb0 if frame[24..28] == [0, 0, 2, 0] => 28,
        0x10 => 26,
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
    let length = 32 + eapol.len();
    let frame = out.get_mut(..length)?;
    management_header(frame, 0x08, station, ap, sequence);
    frame[1] = 1; // To DS: address 1 is the receiver AP, address 3 is destination.
    frame[24..32].copy_from_slice(&[0xaa, 0xaa, 3, 0, 0, 0, 0x88, 0x8e]);
    frame[32..].copy_from_slice(eapol);
    Some(length)
}

/// Extract an unencrypted EAPOL frame sent by the selected access point.
pub fn eapol_reply<'a>(frame: &'a [u8], station: [u8; 6], ap: [u8; 6]) -> Option<&'a [u8]> {
    if frame.len() < 32
        || frame[0] != 0x08
        || frame[1] & 0x43 != 2
        || frame[4..10] != station
        || frame[10..16] != ap
    {
        return None;
    }
    if frame[24..32] != [0xaa, 0xaa, 3, 0, 0, 0, 0x88, 0x8e] {
        return None;
    }
    Some(&frame[32..])
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
        management_header(frame, 0x08, self.station, self.ap, self.sequence);
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
