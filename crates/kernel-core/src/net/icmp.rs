//! ICMP echo request and reply wire-format helpers.

use super::checksum;

pub const ECHO_REPLY: u8 = 0;
pub const ECHO_REQUEST: u8 = 8;
pub const ECHO_CODE: u8 = 0;
pub const ECHO_HEADER_LEN: usize = 8;

#[derive(Debug, Eq, PartialEq)]
pub struct EchoMessage<'a> {
    pub message_type: u8,
    pub code: u8,
    pub identifier: u16,
    pub sequence: u16,
    pub payload: &'a [u8],
}

impl<'a> EchoMessage<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < ECHO_HEADER_LEN || checksum::internet_checksum(bytes) != 0 {
            return None;
        }
        Some(Self {
            message_type: bytes[0],
            code: bytes[1],
            identifier: u16::from_be_bytes([bytes[4], bytes[5]]),
            sequence: u16::from_be_bytes([bytes[6], bytes[7]]),
            payload: &bytes[ECHO_HEADER_LEN..],
        })
    }

    pub fn is_reply_to(&self, identifier: u16, sequence: u16, payload: &[u8]) -> bool {
        self.message_type == ECHO_REPLY
            && self.code == ECHO_CODE
            && self.identifier == identifier
            && self.sequence == sequence
            && self.payload == payload
    }
}

pub fn write_echo_request(
    out: &mut [u8],
    identifier: u16,
    sequence: u16,
    payload: &[u8],
) -> Option<usize> {
    write_echo(out, ECHO_REQUEST, identifier, sequence, payload)
}

pub fn write_echo_reply(
    out: &mut [u8],
    identifier: u16,
    sequence: u16,
    payload: &[u8],
) -> Option<usize> {
    write_echo(out, ECHO_REPLY, identifier, sequence, payload)
}

fn write_echo(
    out: &mut [u8],
    message_type: u8,
    identifier: u16,
    sequence: u16,
    payload: &[u8],
) -> Option<usize> {
    let length = ECHO_HEADER_LEN.checked_add(payload.len())?;
    if out.len() < length {
        return None;
    }
    out[..length].fill(0);
    out[0] = message_type;
    out[1] = ECHO_CODE;
    out[4..6].copy_from_slice(&identifier.to_be_bytes());
    out[6..8].copy_from_slice(&sequence.to_be_bytes());
    out[ECHO_HEADER_LEN..length].copy_from_slice(payload);
    let checksum = checksum::internet_checksum(&out[..length]);
    out[2..4].copy_from_slice(&checksum.to_be_bytes());
    Some(length)
}
