//! Dynamic Host Configuration Protocol (DHCPv4) packets: RFC 2131/2132.
//!
//! DHCP reuses the BOOTP header. We read/write its operation (byte 0), hardware
//! type/length (1/2), transaction ID (4..8), flags (10..12), client address
//! (12..16), offered address (16..20), server address (20..24), and client MAC
//! (28..34). The fixed 236-byte header is followed by the four-byte DHCP cookie.
//! Options follow as code/length/value records; PAD and END have no length.
//! Multi-byte numbers use network order (most significant byte first).

pub const CLIENT_PORT: u16 = 68;
pub const SERVER_PORT: u16 = 67;
pub const DISCOVER: u8 = 1;
pub const OFFER: u8 = 2;
pub const REQUEST: u8 = 3;
pub const ACK: u8 = 5;
pub const NAK: u8 = 6;

const BOOTP_REQUEST: u8 = 1;
const BOOTP_REPLY: u8 = 2;
const HARDWARE_ETHERNET: u8 = 1;
const ETHERNET_ADDRESS_BYTES: u8 = 6;
const REQUEST_BROADCAST_REPLY: u16 = 1 << 15;
// Preserve the existing client padding and output-buffer requirement.
const MIN_MESSAGE_BYTES: usize = 300;

// Option tags from RFC 2132, sections 3 and 9. Names are shared by the
// parser, writer, and parameter-request list so their wire values stay visible.
mod option {
    pub const PAD: u8 = 0;
    pub const SUBNET_MASK: u8 = 1;
    pub const ROUTER: u8 = 3;
    pub const DNS_SERVER: u8 = 6;
    pub const HOST_NAME: u8 = 12;
    pub const REQUESTED_ADDRESS: u8 = 50;
    pub const LEASE_SECONDS: u8 = 51;
    pub const MESSAGE_TYPE: u8 = 53;
    pub const SERVER_IDENTIFIER: u8 = 54;
    pub const PARAMETER_REQUEST_LIST: u8 = 55;
    pub const RENEWAL_SECONDS: u8 = 58;
    pub const REBINDING_SECONDS: u8 = 59;
    pub const CLIENT_IDENTIFIER: u8 = 61;
    pub const END: u8 = 255;
}

const BOOTP_FIXED_LEN: usize = 236;
const OPTIONS_START: usize = 240;
const MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DhcpReply {
    pub message_type: u8,
    pub address: [u8; 4],
    pub server: [u8; 4],
    pub subnet_mask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns: [u8; 4],
    pub lease_seconds: u32,
    pub renewal_seconds: u32,
    pub rebinding_seconds: u32,
}

pub struct DhcpClient {
    mac: [u8; 6],
    transaction_id: u32,
}

impl DhcpClient {
    pub const fn new(mac: [u8; 6], transaction_id: u32) -> Self {
        Self {
            mac,
            transaction_id,
        }
    }

    pub fn write_discover(&self, out: &mut [u8]) -> Option<usize> {
        self.write_message(out, DISCOVER, None, None)
    }

    pub fn write_request(&self, out: &mut [u8], offer: &DhcpReply) -> Option<usize> {
        self.write_message(out, REQUEST, Some(offer.address), Some(offer.server))
    }

    /// Renewal uses ciaddr and omits requested-address/server options (RFC 2131 §4.3.2).
    pub fn write_renewal(&self, out: &mut [u8], address: [u8; 4]) -> Option<usize> {
        let length = self.write_message(out, REQUEST, None, None)?;
        out[10..12].fill(0);
        out[12..16].copy_from_slice(&address);
        Some(length)
    }

    pub fn restart(&mut self) {
        self.transaction_id = self.transaction_id.wrapping_add(1);
    }

    pub fn parse_reply(&self, bytes: &[u8]) -> Option<DhcpReply> {
        if bytes.len() <= OPTIONS_START
            || bytes[0] != BOOTP_REPLY
            || bytes[1] != HARDWARE_ETHERNET
            || bytes[2] != ETHERNET_ADDRESS_BYTES
            || bytes[4..8] != self.transaction_id.to_be_bytes()
            || bytes[28..34] != self.mac
            || bytes[BOOTP_FIXED_LEN..OPTIONS_START] != MAGIC_COOKIE
        {
            return None;
        }

        let mut reply = DhcpReply {
            message_type: 0,
            address: bytes[16..20].try_into().ok()?,
            server: bytes[20..24].try_into().ok()?,
            subnet_mask: [0; 4],
            gateway: [0; 4],
            dns: [0; 4],
            lease_seconds: 0,
            renewal_seconds: 0,
            rebinding_seconds: 0,
        };
        let mut offset = OPTIONS_START;
        while offset < bytes.len() {
            let code = bytes[offset];
            offset += 1;
            if code == option::END {
                break;
            }
            if code == option::PAD {
                continue;
            }
            let length = *bytes.get(offset)? as usize;
            offset += 1;
            let value = bytes.get(offset..offset.checked_add(length)?)?;
            match (code, length) {
                (option::MESSAGE_TYPE, 1) => reply.message_type = value[0],
                (option::SERVER_IDENTIFIER, 4) => reply.server.copy_from_slice(value),
                (option::SUBNET_MASK, 4) => reply.subnet_mask.copy_from_slice(value),
                (option::ROUTER, n) if n >= 4 && n % 4 == 0 => {
                    reply.gateway.copy_from_slice(&value[..4])
                }
                (option::DNS_SERVER, n) if n >= 4 && n % 4 == 0 => {
                    reply.dns.copy_from_slice(&value[..4])
                }
                (option::LEASE_SECONDS, 4) => {
                    reply.lease_seconds = u32::from_be_bytes(value.try_into().ok()?)
                }
                (option::RENEWAL_SECONDS, 4) => {
                    reply.renewal_seconds = u32::from_be_bytes(value.try_into().ok()?)
                }
                (option::REBINDING_SECONDS, 4) => {
                    reply.rebinding_seconds = u32::from_be_bytes(value.try_into().ok()?)
                }
                _ => {}
            }
            offset += length;
        }
        if !matches!(reply.message_type, OFFER | ACK | NAK) {
            None
        } else {
            Some(reply)
        }
    }

    fn write_message(
        &self,
        out: &mut [u8],
        message_type: u8,
        requested_address: Option<[u8; 4]>,
        server: Option<[u8; 4]>,
    ) -> Option<usize> {
        if out.len() < MIN_MESSAGE_BYTES {
            return None;
        }
        out.fill(0);
        out[0] = BOOTP_REQUEST;
        out[1] = HARDWARE_ETHERNET;
        out[2] = ETHERNET_ADDRESS_BYTES;
        out[4..8].copy_from_slice(&self.transaction_id.to_be_bytes());
        out[10..12].copy_from_slice(&REQUEST_BROADCAST_REPLY.to_be_bytes());
        out[28..34].copy_from_slice(&self.mac);
        out[BOOTP_FIXED_LEN..OPTIONS_START].copy_from_slice(&MAGIC_COOKIE);

        let mut offset = OPTIONS_START;
        write_option(out, &mut offset, option::MESSAGE_TYPE, &[message_type])?;
        if let Some(address) = requested_address {
            write_option(out, &mut offset, option::REQUESTED_ADDRESS, &address)?;
        }
        if let Some(address) = server {
            write_option(out, &mut offset, option::SERVER_IDENTIFIER, &address)?;
        }
        write_option(
            out,
            &mut offset,
            option::PARAMETER_REQUEST_LIST,
            &[
                option::SUBNET_MASK,
                option::ROUTER,
                option::DNS_SERVER,
                option::LEASE_SECONDS,
                option::SERVER_IDENTIFIER,
                option::RENEWAL_SECONDS,
                option::REBINDING_SECONDS,
            ],
        )?;
        write_option(out, &mut offset, option::HOST_NAME, b"gabix-pi2")?;
        // Client identifier: one hardware-type byte, then our six-byte MAC.
        let mut identity = [HARDWARE_ETHERNET; 1 + ETHERNET_ADDRESS_BYTES as usize];
        identity[1..].copy_from_slice(&self.mac);
        write_option(out, &mut offset, option::CLIENT_IDENTIFIER, &identity)?;
        *out.get_mut(offset)? = option::END;
        Some((offset + 1).max(MIN_MESSAGE_BYTES))
    }
}

fn write_option(out: &mut [u8], offset: &mut usize, code: u8, value: &[u8]) -> Option<()> {
    let end = offset.checked_add(2)?.checked_add(value.len())?;
    if value.len() > u8::MAX as usize || end > out.len() {
        return None;
    }
    out[*offset] = code;
    out[*offset + 1] = value.len() as u8;
    out[*offset + 2..end].copy_from_slice(value);
    *offset = end;
    Some(())
}
