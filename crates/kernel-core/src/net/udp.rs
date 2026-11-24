//! RFC 768 UDP parsing, building, and IPv4 checksum validation.
//!
//! The eight-byte header contains four big-endian (high-byte-first) u16 fields:
//! bytes 0..2 source port, 2..4 destination port, 4..6 total datagram length,
//! 6..8 checksum. Payload follows at byte 8.
//!
//! The checksum also covers an IPv4 pseudo-header: source IP (4 bytes),
//! destination IP (4), zero (1), protocol (1), and UDP length (2). These extra
//! bytes are checksum inputs; they are not prepended to the UDP datagram.

use super::checksum;

pub const UDP_HEADER_LEN: usize = 8;
pub const PROTOCOL_UDP: u8 = 17;
pub const ECHO_PORT: u16 = 5555;

// RFC 768: zero on the wire means the sender omitted the IPv4 UDP checksum.
// Encode an actually computed zero checksum as all ones to distinguish it.
const CHECKSUM_OMITTED: u16 = 0;
const COMPUTED_ZERO_CHECKSUM: u16 = u16::MAX;
const IPV4_PSEUDO_HEADER_LEN: usize = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
}

#[derive(Debug, Eq, PartialEq)]
pub struct UdpDatagram<'a> {
    pub header: UdpHeader,
    pub payload: &'a [u8],
}

impl<'a> UdpDatagram<'a> {
    pub fn parse(bytes: &'a [u8], src_ip: [u8; 4], dst_ip: [u8; 4]) -> Option<Self> {
        if bytes.len() < UDP_HEADER_LEN {
            return None;
        }
        let length = u16::from_be_bytes([bytes[4], bytes[5]]);
        if length < UDP_HEADER_LEN as u16 || length as usize > bytes.len() {
            return None;
        }
        let header = UdpHeader {
            src_port: u16::from_be_bytes([bytes[0], bytes[1]]),
            dst_port: u16::from_be_bytes([bytes[2], bytes[3]]),
            length,
            checksum: u16::from_be_bytes([bytes[6], bytes[7]]),
        };
        if header.checksum != CHECKSUM_OMITTED
            && ipv4_checksum(src_ip, dst_ip, &bytes[..length as usize]) != 0
        {
            return None;
        }
        Some(Self {
            header,
            payload: &bytes[UDP_HEADER_LEN..length as usize],
        })
    }
}

pub fn write_datagram(
    out: &mut [u8],
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> Option<usize> {
    let length = UDP_HEADER_LEN.checked_add(payload.len())?;
    if length > u16::MAX as usize || out.len() < length {
        return None;
    }
    out[..length].fill(0);
    out[0..2].copy_from_slice(&src_port.to_be_bytes());
    out[2..4].copy_from_slice(&dst_port.to_be_bytes());
    out[4..6].copy_from_slice(&(length as u16).to_be_bytes());
    out[UDP_HEADER_LEN..length].copy_from_slice(payload);

    let mut pseudo_header = [0u8; IPV4_PSEUDO_HEADER_LEN];
    pseudo_header[0..4].copy_from_slice(&src_ip);
    pseudo_header[4..8].copy_from_slice(&dst_ip);
    pseudo_header[9] = PROTOCOL_UDP;
    pseudo_header[10..12].copy_from_slice(&(length as u16).to_be_bytes());
    let checksum = checksum::internet_checksum_parts(&[&pseudo_header, &out[..length]]);
    out[6..8].copy_from_slice(
        &(if checksum == 0 {
            COMPUTED_ZERO_CHECKSUM
        } else {
            checksum
        })
        .to_be_bytes(),
    );
    Some(length)
}

fn ipv4_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], udp_bytes: &[u8]) -> u16 {
    let mut pseudo_header = [0u8; IPV4_PSEUDO_HEADER_LEN];
    pseudo_header[0..4].copy_from_slice(&src_ip);
    pseudo_header[4..8].copy_from_slice(&dst_ip);
    pseudo_header[9] = PROTOCOL_UDP;
    pseudo_header[10..12].copy_from_slice(&(udp_bytes.len() as u16).to_be_bytes());
    checksum::internet_checksum_parts(&[&pseudo_header, udp_bytes])
}
