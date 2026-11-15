//! DHCPv4 client packet construction and reply parsing.

pub const CLIENT_PORT: u16 = 68;
pub const SERVER_PORT: u16 = 67;
pub const DISCOVER: u8 = 1;
pub const OFFER: u8 = 2;
pub const REQUEST: u8 = 3;
pub const ACK: u8 = 5;
pub const NAK: u8 = 6;

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
            || bytes[0] != 2
            || bytes[1] != 1
            || bytes[2] != 6
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
            if code == 255 {
                break;
            }
            if code == 0 {
                continue;
            }
            let length = *bytes.get(offset)? as usize;
            offset += 1;
            let value = bytes.get(offset..offset.checked_add(length)?)?;
            match (code, length) {
                (53, 1) => reply.message_type = value[0],
                (54, 4) => reply.server.copy_from_slice(value),
                (1, 4) => reply.subnet_mask.copy_from_slice(value),
                (3, n) if n >= 4 && n % 4 == 0 => reply.gateway.copy_from_slice(&value[..4]),
                (6, n) if n >= 4 && n % 4 == 0 => reply.dns.copy_from_slice(&value[..4]),
                (51, 4) => reply.lease_seconds = u32::from_be_bytes(value.try_into().ok()?),
                (58, 4) => reply.renewal_seconds = u32::from_be_bytes(value.try_into().ok()?),
                (59, 4) => reply.rebinding_seconds = u32::from_be_bytes(value.try_into().ok()?),
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
        if out.len() < 300 {
            return None;
        }
        out.fill(0);
        out[0] = 1;
        out[1] = 1;
        out[2] = 6;
        out[4..8].copy_from_slice(&self.transaction_id.to_be_bytes());
        out[10..12].copy_from_slice(&0x8000u16.to_be_bytes());
        out[28..34].copy_from_slice(&self.mac);
        out[BOOTP_FIXED_LEN..OPTIONS_START].copy_from_slice(&MAGIC_COOKIE);

        let mut offset = OPTIONS_START;
        write_option(out, &mut offset, 53, &[message_type])?;
        if let Some(address) = requested_address {
            write_option(out, &mut offset, 50, &address)?;
        }
        if let Some(address) = server {
            write_option(out, &mut offset, 54, &address)?;
        }
        write_option(out, &mut offset, 55, &[1, 3, 6, 51, 54, 58, 59])?;
        write_option(out, &mut offset, 12, b"gabix-pi2")?;
        let mut identity = [1u8; 7];
        identity[1..].copy_from_slice(&self.mac);
        write_option(out, &mut offset, 61, &identity)?;
        *out.get_mut(offset)? = 255;
        Some((offset + 1).max(300))
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
