//! RFC 768 UDP parsing, building, and IPv4 checksum validation.

use crate::net::checksum;

pub const UDP_HEADER_LEN: usize = 8;
pub const PROTOCOL_UDP: u8 = 17;
pub const ECHO_PORT: u16 = 5555;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
}

/// A checksum-verified UDP datagram borrowed from an IPv4 payload.
#[derive(Debug, Eq, PartialEq)]
pub struct UdpDatagram<'a> {
    pub header: UdpHeader,
    pub payload: &'a [u8],
}

impl<'a> UdpDatagram<'a> {
    /// Parse one UDP datagram inside the IPv4 payload boundary.
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

        if header.checksum != 0 {
            let checksum = ipv4_checksum(src_ip, dst_ip, &bytes[..length as usize]);
            if checksum != 0 {
                return None;
            }
        }

        Some(Self {
            header,
            payload: &bytes[UDP_HEADER_LEN..length as usize],
        })
    }
}

/// Write one UDP datagram and return its wire length.
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

    for byte in &mut out[..length] {
        *byte = 0;
    }
    out[0..2].copy_from_slice(&src_port.to_be_bytes());
    out[2..4].copy_from_slice(&dst_port.to_be_bytes());
    out[4..6].copy_from_slice(&(length as u16).to_be_bytes());
    out[UDP_HEADER_LEN..length].copy_from_slice(payload);

    let mut pseudo_header = [0u8; 12];
    pseudo_header[0..4].copy_from_slice(&src_ip);
    pseudo_header[4..8].copy_from_slice(&dst_ip);
    pseudo_header[8] = 0;
    pseudo_header[9] = PROTOCOL_UDP;
    pseudo_header[10..12].copy_from_slice(&(length as u16).to_be_bytes());

    let checksum = checksum::internet_checksum_parts(&[&pseudo_header, &out[..length]]);
    let transmitted_checksum = if checksum == 0 { 0xffff } else { checksum };
    out[6..8].copy_from_slice(&transmitted_checksum.to_be_bytes());
    Some(length)
}

fn ipv4_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], udp_bytes: &[u8]) -> u16 {
    let mut pseudo_header = [0u8; 12];
    pseudo_header[0..4].copy_from_slice(&src_ip);
    pseudo_header[4..8].copy_from_slice(&dst_ip);
    pseudo_header[8] = 0;
    pseudo_header[9] = PROTOCOL_UDP;
    pseudo_header[10..12].copy_from_slice(&(udp_bytes.len() as u16).to_be_bytes());
    checksum::internet_checksum_parts(&[&pseudo_header, udp_bytes])
}
