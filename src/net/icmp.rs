//! ICMP echo request and reply wire-format helpers.
//!
//! ICMP has no length field of its own. IPv4 gives us the message boundary;
//! this module owns the eight bytes inside that boundary that identify echo.

use crate::net::checksum;

pub const ECHO_REPLY: u8 = 0;
pub const ECHO_REQUEST: u8 = 8;
pub const ECHO_CODE: u8 = 0;
pub const ECHO_HEADER_LEN: usize = 8;

/// A checksum-verified ICMP echo message borrowed from its IPv4 payload.
#[derive(Debug, Eq, PartialEq)]
pub struct EchoMessage<'a> {
    pub message_type: u8,
    pub code: u8,
    pub identifier: u16,
    pub sequence: u16,
    pub payload: &'a [u8],
}

impl<'a> EchoMessage<'a> {
    /// Parse one complete ICMP message after IPv4 has supplied its boundary.
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < ECHO_HEADER_LEN {
            return None;
        }
        if checksum::internet_checksum(bytes) != 0 {
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

    /// Decide whether this message proves the supplied request completed.
    pub fn is_reply_to(&self, identifier: u16, sequence: u16, payload: &[u8]) -> bool {
        self.message_type == ECHO_REPLY
            && self.code == ECHO_CODE
            && self.identifier == identifier
            && self.sequence == sequence
            && self.payload == payload
    }
}

/// Write an ICMP echo request into `out`, returning the message length.
///
/// The caller chooses the identifier, sequence, and data. The destination
/// returns those three values unchanged in its echo reply.
pub fn write_echo_request(
    out: &mut [u8],
    identifier: u16,
    sequence: u16,
    payload: &[u8],
) -> Option<usize> {
    write_echo(out, ECHO_REQUEST, identifier, sequence, payload)
}

/// Write an ICMP echo reply that preserves the request's conversation data.
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
    let message_len = ECHO_HEADER_LEN.checked_add(payload.len())?;
    if out.len() < message_len {
        return None;
    }

    for byte in &mut out[..message_len] {
        *byte = 0;
    }
    out[0] = message_type;
    out[1] = ECHO_CODE;
    out[4..6].copy_from_slice(&identifier.to_be_bytes());
    out[6..8].copy_from_slice(&sequence.to_be_bytes());
    out[ECHO_HEADER_LEN..message_len].copy_from_slice(payload);

    let checksum = checksum::internet_checksum(&out[..message_len]);
    out[2..4].copy_from_slice(&checksum.to_be_bytes());
    Some(message_len)
}
