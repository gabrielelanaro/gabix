//! Authenticate each management datagram and reject replay across normal boots.
//! Duplicate retries return the cached reply without executing the command twice.
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
const KEY: &[u8; 32] = include_bytes!(env!("GABIX_PI_MANAGEMENT_KEY"));
pub const OVERHEAD: usize = 52; // magic + boot number + sequence + HMAC-SHA256
pub const MAX_REPLY: usize = 532 + OVERHEAD;

pub struct Authentication {
    pub boot: u64,
    pub sequence: u64,
    pub rejected: u32,
    last_tag: [u8; 32],
    reply: [u8; MAX_REPLY],
    reply_len: usize,
}
pub enum Request<'a> {
    Fresh(&'a [u8]),
    Retry,
    Rejected,
}
impl Authentication {
    pub fn new(boot: u64) -> Self {
        Self {
            boot,
            sequence: 0,
            rejected: 0,
            last_tag: [0; 32],
            reply: [0; MAX_REPLY],
            reply_len: 0,
        }
    }
    pub fn verify<'a>(&mut self, packet: &'a [u8]) -> Request<'a> {
        let result = (|| {
            if packet.len() < OVERHEAD + 20 || &packet[..4] != b"GBAU" {
                return None;
            }
            let boot = u64::from_le_bytes(packet[4..12].try_into().ok()?);
            let sequence = u64::from_le_bytes(packet[12..20].try_into().ok()?);
            let end = packet.len() - 32;
            let mut mac = Hmac::<Sha256>::new_from_slice(KEY).ok()?;
            mac.update(b"request");
            mac.update(&packet[..end]);
            mac.verify_slice(&packet[end..]).ok()?;
            if boot != self.boot {
                return None;
            }
            if sequence == self.sequence && packet[end..] == self.last_tag && self.reply_len != 0 {
                return Some(Request::Retry);
            }
            if sequence != self.sequence.checked_add(1)? {
                return None;
            }
            self.sequence = sequence;
            self.last_tag.copy_from_slice(&packet[end..]);
            self.reply_len = 0;
            Some(Request::Fresh(&packet[20..end]))
        })();
        result.unwrap_or_else(|| {
            self.rejected = self.rejected.saturating_add(1);
            Request::Rejected
        })
    }
    pub fn seal(&mut self, inner: &[u8]) {
        let length = 20 + inner.len();
        self.reply[..4].copy_from_slice(b"GBAU");
        self.reply[4..12].copy_from_slice(&self.boot.to_le_bytes());
        self.reply[12..20].copy_from_slice(&self.sequence.to_le_bytes());
        self.reply[20..length].copy_from_slice(inner);
        let mut mac = Hmac::<Sha256>::new_from_slice(KEY).unwrap();
        mac.update(b"response");
        mac.update(&self.reply[..length]);
        self.reply[length..length + 32].copy_from_slice(&mac.finalize().into_bytes());
        self.reply_len = length + 32;
    }
    pub fn reply(&self, out: &mut [u8]) -> usize {
        out[..self.reply_len].copy_from_slice(&self.reply[..self.reply_len]);
        self.reply_len
    }
}
