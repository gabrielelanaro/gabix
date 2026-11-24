//! IPv4 TCP segment encoding and parsing shared by the kernel targets.

use super::checksum;

pub const PROTOCOL_TCP: u8 = 6;
pub const TCP_HEADER_LEN: usize = 20;
pub const FLAG_FIN: u8 = 0x01;
pub const FLAG_SYN: u8 = 0x02;
pub const FLAG_RST: u8 = 0x04;
pub const FLAG_PSH: u8 = 0x08;
pub const FLAG_ACK: u8 = 0x10;

// RFC 9293 section 3.1: byte ranges in the fixed TCP header.
mod header_field {
    pub const SOURCE_PORT: core::ops::Range<usize> = 0..2;
    pub const DESTINATION_PORT: core::ops::Range<usize> = 2..4;
    pub const SEQUENCE: core::ops::Range<usize> = 4..8;
    pub const ACKNOWLEDGMENT: core::ops::Range<usize> = 8..12;
    pub const DATA_OFFSET: usize = 12;
    pub const FLAGS: usize = 13;
    pub const WINDOW: core::ops::Range<usize> = 14..16;
    pub const CHECKSUM: core::ops::Range<usize> = 16..18;
}
const HEADER_WORD_BYTES: usize = 4;
const DATA_OFFSET_SHIFT: u32 = 4;
// Advertised values are fixed policy here, independent of receive-queue usage.
const ADVERTISED_RECEIVE_WINDOW: u16 = 4096;
const ADVERTISED_MSS: u16 = 1460;
const MSS_OPTION_KIND: u8 = 2;
const MSS_OPTION_BYTES: usize = 4;
pub const SYN_HEADER_BYTES: usize = TCP_HEADER_LEN + MSS_OPTION_BYTES;

// IPv4 pseudo-header used only for checksum calculation, not transmitted here.
mod pseudo_header {
    pub const BYTE_LEN: usize = 12;
    pub const SOURCE_IP: core::ops::Range<usize> = 0..4;
    pub const DESTINATION_IP: core::ops::Range<usize> = 4..8;
    pub const PROTOCOL: usize = 9;
    pub const TCP_LENGTH: core::ops::Range<usize> = 10..12;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub sequence: u32,
    pub acknowledgment: u32,
    pub flags: u8,
    pub window: u16,
    pub header_len: usize,
}

pub struct TcpSegment<'a> {
    pub header: TcpHeader,
    pub payload: &'a [u8],
}

impl<'a> TcpSegment<'a> {
    /// Validate minimum/header lengths and the checksum over all supplied
    /// bytes. Options are skipped using Data Offset; their contents and lengths
    /// are not interpreted or negotiated here.
    pub fn parse(bytes: &'a [u8], src_ip: [u8; 4], dst_ip: [u8; 4]) -> Option<Self> {
        if bytes.len() < TCP_HEADER_LEN || tcp_checksum(src_ip, dst_ip, bytes) != 0 {
            return None;
        }
        let header_len =
            ((bytes[header_field::DATA_OFFSET] >> DATA_OFFSET_SHIFT) as usize) * HEADER_WORD_BYTES;
        if header_len < TCP_HEADER_LEN || header_len > bytes.len() {
            return None;
        }
        Some(Self {
            header: TcpHeader {
                src_port: u16::from_be_bytes(bytes[header_field::SOURCE_PORT].try_into().ok()?),
                dst_port: u16::from_be_bytes(
                    bytes[header_field::DESTINATION_PORT].try_into().ok()?,
                ),
                sequence: u32::from_be_bytes(bytes[header_field::SEQUENCE].try_into().ok()?),
                acknowledgment: u32::from_be_bytes(
                    bytes[header_field::ACKNOWLEDGMENT].try_into().ok()?,
                ),
                flags: bytes[header_field::FLAGS],
                window: u16::from_be_bytes(bytes[header_field::WINDOW].try_into().ok()?),
                header_len,
            },
            payload: &bytes[header_len..],
        })
    }
}

/// Write a TCP segment. SYN segments include the four-byte MSS option.
pub fn write_segment(
    out: &mut [u8],
    endpoints: Endpoints,
    sequence: u32,
    acknowledgment: u32,
    flags: u8,
    payload: &[u8],
) -> Option<usize> {
    write_segment_with_window(
        out,
        endpoints,
        sequence,
        acknowledgment,
        flags,
        payload,
        ADVERTISED_RECEIVE_WINDOW,
    )
}

pub fn write_segment_with_window(
    out: &mut [u8],
    endpoints: Endpoints,
    sequence: u32,
    acknowledgment: u32,
    flags: u8,
    payload: &[u8],
    receive_window: u16,
) -> Option<usize> {
    let header_len = if flags & FLAG_SYN != 0 {
        SYN_HEADER_BYTES
    } else {
        TCP_HEADER_LEN
    };
    let length = header_len.checked_add(payload.len())?;
    if length > u16::MAX as usize || out.len() < length {
        return None;
    }
    out[..length].fill(0);
    out[header_field::SOURCE_PORT].copy_from_slice(&endpoints.local_port.to_be_bytes());
    out[header_field::DESTINATION_PORT].copy_from_slice(&endpoints.remote_port.to_be_bytes());
    out[header_field::SEQUENCE].copy_from_slice(&sequence.to_be_bytes());
    out[header_field::ACKNOWLEDGMENT].copy_from_slice(&acknowledgment.to_be_bytes());
    out[header_field::DATA_OFFSET] = ((header_len / HEADER_WORD_BYTES) as u8) << DATA_OFFSET_SHIFT;
    out[header_field::FLAGS] = flags;
    out[header_field::WINDOW].copy_from_slice(&receive_window.to_be_bytes());
    if header_len == SYN_HEADER_BYTES {
        let mss_bytes = ADVERTISED_MSS.to_be_bytes();
        out[TCP_HEADER_LEN..SYN_HEADER_BYTES].copy_from_slice(&[
            MSS_OPTION_KIND,
            MSS_OPTION_BYTES as u8,
            mss_bytes[0],
            mss_bytes[1],
        ]);
    }
    out[header_len..length].copy_from_slice(payload);
    let sum = tcp_checksum(endpoints.local_ip, endpoints.remote_ip, &out[..length]);
    out[header_field::CHECKSUM].copy_from_slice(&sum.to_be_bytes());
    Some(length)
}

fn tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], bytes: &[u8]) -> u16 {
    let mut pseudo = [0u8; pseudo_header::BYTE_LEN];
    pseudo[pseudo_header::SOURCE_IP].copy_from_slice(&src_ip);
    pseudo[pseudo_header::DESTINATION_IP].copy_from_slice(&dst_ip);
    pseudo[pseudo_header::PROTOCOL] = PROTOCOL_TCP;
    pseudo[pseudo_header::TCP_LENGTH].copy_from_slice(&(bytes.len() as u16).to_be_bytes());
    checksum::internet_checksum_parts(&[&pseudo, bytes])
}

#[derive(Clone, Copy)]
pub struct Endpoints {
    pub local_ip: [u8; 4],
    pub remote_ip: [u8; 4],
    pub local_port: u16,
    pub remote_port: u16,
}
