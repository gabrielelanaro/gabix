//! Authenticate each management datagram and reject replay across normal boots.
//! Duplicate retries return the cached reply without executing the command twice.
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
const KEY: &[u8; 32] = include_bytes!(env!("GABIX_PI_MANAGEMENT_KEY"));
const MAGIC: &[u8; 4] = b"GBAU";
const PREFIX_BYTES: usize = 20;
const TAG_BYTES: usize = 32;
const MIN_COMMAND_BYTES: usize = 20;
const MAX_COMMAND_REPLY_BYTES: usize = 532;
const BOOT_FIELD: core::ops::Range<usize> = 4..12;
const SEQUENCE_FIELD: core::ops::Range<usize> = 12..20;
// Different prefixes prevent a signed response from being reused as a request.
const REQUEST_DOMAIN: &[u8] = b"request";
const RESPONSE_DOMAIN: &[u8] = b"response";
pub const OVERHEAD: usize = PREFIX_BYTES + TAG_BYTES;
pub const MAX_REPLY: usize = MAX_COMMAND_REPLY_BYTES + OVERHEAD;

pub struct Authentication {
    pub boot: u64,
    pub sequence: u64,
    pub rejected: u32,
    last_tag: [u8; TAG_BYTES],
    reply: [u8; MAX_REPLY],
    reply_len: usize,
}
pub enum Request<'a> {
    Fresh(&'a [u8]),
    Retry,
    Rejected,
}
impl Authentication {
    /// boot must come from the persistent boot counter. Replay protection across
    /// resets depends on that counter not being reused (for example after rollback).
    pub fn new(boot: u64) -> Self {
        Self {
            boot,
            sequence: 0,
            rejected: 0,
            last_tag: [0; TAG_BYTES],
            reply: [0; MAX_REPLY],
            reply_len: 0,
        }
    }
    /// Authenticate before checking boot/sequence. Fresh requests consume the
    /// next sequence immediately, before the caller executes their command.
    /// Only the most recent tag/reply is cached; all peers share this counter.
    /// If the caller never seals a reply, a same-sequence retry is rejected.
    pub fn verify<'a>(&mut self, packet: &'a [u8]) -> Request<'a> {
        let result = (|| {
            if packet.len() < OVERHEAD + MIN_COMMAND_BYTES || &packet[..MAGIC.len()] != MAGIC {
                return None;
            }
            let boot = u64::from_le_bytes(packet[BOOT_FIELD].try_into().ok()?);
            let sequence = u64::from_le_bytes(packet[SEQUENCE_FIELD].try_into().ok()?);
            let end = packet.len() - TAG_BYTES;
            let mut mac = Hmac::<Sha256>::new_from_slice(KEY).ok()?;
            mac.update(REQUEST_DOMAIN);
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
            Some(Request::Fresh(&packet[PREFIX_BYTES..end]))
        })();
        result.unwrap_or_else(|| {
            self.rejected = self.rejected.saturating_add(1);
            Request::Rejected
        })
    }
    /// Cache a signed reply for the current sequence after command execution.
    /// Caller must keep inner within MAX_COMMAND_REPLY_BYTES; larger input panics.
    pub fn seal(&mut self, inner: &[u8]) {
        let length = PREFIX_BYTES + inner.len();
        self.reply[..MAGIC.len()].copy_from_slice(MAGIC);
        self.reply[BOOT_FIELD].copy_from_slice(&self.boot.to_le_bytes());
        self.reply[SEQUENCE_FIELD].copy_from_slice(&self.sequence.to_le_bytes());
        self.reply[PREFIX_BYTES..length].copy_from_slice(inner);
        let mut mac = Hmac::<Sha256>::new_from_slice(KEY).unwrap();
        mac.update(RESPONSE_DOMAIN);
        mac.update(&self.reply[..length]);
        self.reply[length..length + TAG_BYTES].copy_from_slice(&mac.finalize().into_bytes());
        self.reply_len = length + TAG_BYTES;
    }
    /// Copy the cached bytes; out must hold reply_len bytes or this panics.
    pub fn reply(&self, out: &mut [u8]) -> usize {
        out[..self.reply_len].copy_from_slice(&self.reply[..self.reply_len]);
        self.reply_len
    }
}
