//! IPv4 header parsing and building (RFC 791).
//!
//! This implementation accepts the fixed 20-byte header and unfragmented data.
//! Byte 0 packs version in the high four bits and header length in the low
//! four bits. Header length is measured in 32-bit words, so five means 20 bytes.
//! Remaining fields: total length (2..4), identification (4..6), flags/fragment
//! offset (6..8), hop limit/TTL (8), payload protocol (9), checksum (10..12),
//! source address (12..16), destination address (16..20). u16 fields are big-endian.

use super::checksum;

pub const IPV4_HEADER_LEN: usize = 20;
const VERSION: u8 = 4;
const HEADER_WORDS_WITHOUT_OPTIONS: u8 = 5;
const VERSION_SHIFT: u8 = 4;
const HEADER_WORD_COUNT_MASK: u8 = 0x0f;
pub const VERSION_IHL_NO_OPTIONS: u8 = (VERSION << VERSION_SHIFT) | HEADER_WORDS_WITHOUT_OPTIONS;
pub const DEFAULT_TTL: u8 = 64;
pub const FLAG_DONT_FRAGMENT: u16 = 0x4000;
pub const FLAG_MORE_FRAGMENTS: u16 = 0x2000;
pub const FRAGMENT_OFFSET_MASK: u16 = 0x1fff;
pub const PROTOCOL_ICMP: u8 = 1;
pub const PROTOCOL_TCP: u8 = 6;
pub const PROTOCOL_UDP: u8 = 17;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ipv4Header {
    pub total_len: u16,
    pub identification: u16,
    pub flags_fragment: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub checksum: u16,
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
}

impl Ipv4Header {
    pub fn parse(packet: &[u8]) -> Option<Self> {
        if packet.len() < IPV4_HEADER_LEN {
            return None;
        }
        let version = packet[0] >> VERSION_SHIFT;
        let ihl = packet[0] & HEADER_WORD_COUNT_MASK;
        if version != VERSION || ihl != HEADER_WORDS_WITHOUT_OPTIONS {
            return None;
        }
        let total_len = u16::from_be_bytes([packet[2], packet[3]]);
        if total_len < IPV4_HEADER_LEN as u16 || total_len as usize > packet.len() {
            return None;
        }
        if checksum::internet_checksum(&packet[..IPV4_HEADER_LEN]) != 0 {
            return None;
        }
        let flags_fragment = u16::from_be_bytes([packet[6], packet[7]]);
        if flags_fragment & (FLAG_MORE_FRAGMENTS | FRAGMENT_OFFSET_MASK) != 0 {
            return None;
        }
        let mut src_ip = [0u8; 4];
        let mut dst_ip = [0u8; 4];
        src_ip.copy_from_slice(&packet[12..16]);
        dst_ip.copy_from_slice(&packet[16..20]);
        Some(Self {
            total_len,
            identification: u16::from_be_bytes([packet[4], packet[5]]),
            flags_fragment,
            ttl: packet[8],
            protocol: packet[9],
            checksum: u16::from_be_bytes([packet[10], packet[11]]),
            src_ip,
            dst_ip,
        })
    }

    pub fn build(
        protocol: u8,
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        payload_len: usize,
    ) -> [u8; IPV4_HEADER_LEN] {
        let total_len = IPV4_HEADER_LEN
            .checked_add(payload_len)
            .expect("IPv4 packet length overflow");
        assert!(total_len <= u16::MAX as usize);
        let mut header = [0u8; IPV4_HEADER_LEN];
        header[0] = VERSION_IHL_NO_OPTIONS;
        header[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
        header[6..8].copy_from_slice(&FLAG_DONT_FRAGMENT.to_be_bytes());
        header[8] = DEFAULT_TTL;
        header[9] = protocol;
        header[12..16].copy_from_slice(&src_ip);
        header[16..20].copy_from_slice(&dst_ip);
        let checksum = checksum::internet_checksum(&header);
        header[10..12].copy_from_slice(&checksum.to_be_bytes());
        header
    }

    pub fn payload_len(&self) -> usize {
        self.total_len as usize - IPV4_HEADER_LEN
    }
}
