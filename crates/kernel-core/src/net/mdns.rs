//! IPv4 host-name discovery (RFC 6762, sections 6–10 and 18).
//!
//! Claim gabix-pi2.local after three probes, announce its DHCP address, and
//! answer A/ANY questions. A conflicting address makes us probe a suffixed
//! name. This module owns DNS packets; FrameDevice owns packet delivery.
//! Service browsing (DNS-SD), IPv6 and reverse lookup are not implemented.
//!
//! DNS starts with six big-endian u16 fields: transaction ID, flags, and the
//! question/answer/authority/additional counts (12 bytes total). Each question
//! contains a label-encoded name, type and class. Each resource record adds a
//! 32-bit cache lifetime, a 16-bit data length, and the record's data bytes.

use super::{
    checksum,
    ethernet::{EthernetHeader, ETHERNET_HEADER_LEN, ETHERTYPE_IPV4},
    ipv4::{Ipv4Header, IPV4_HEADER_LEN, PROTOCOL_UDP},
    stack::{NetworkInterface, MAX_FRAME_LEN},
    udp::UdpDatagram,
};
use crate::device::FrameDevice;

pub const PORT: u16 = 5353;
pub const MULTICAST_IP: [u8; 4] = [224, 0, 0, 251];
pub const MULTICAST_MAC: [u8; 6] = [1, 0, 0x5e, 0, 0, 0xfb];
const DNS_HEADER_BYTES: usize = 12;
const TYPE_A: u16 = 1;
const TYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
const UNICAST_OR_CACHE_FLUSH: u16 = 0x8000;
const RESPONSE: u16 = 0x8000;
const AUTHORITATIVE: u16 = 0x0400;
const RECORD_TTL_SECONDS: u32 = 120;
const LEGACY_TTL_SECONDS: u32 = 10;
const LINK_LOCAL_HOP_LIMIT: u8 = 255;
const PROBE_INTERVAL_MS: u64 = 250;
const ANNOUNCEMENT_INTERVAL_MS: u64 = 1000;
const PROBE_COUNT: u8 = 3;
const ANNOUNCEMENT_COUNT: u8 = 2;
const DNS_BUFFER_BYTES: usize = 512;

pub struct Host {
    address: [u8; 4],
    suffix: u16,
    probes: u8,
    announcements: u8,
    deadline: u64,
    now: u64,
}

impl Host {
    pub const fn new() -> Self {
        Self {
            address: [0; 4],
            suffix: 0,
            probes: 0,
            announcements: 0,
            deadline: 0,
            now: 0,
        }
    }

    /// Call even when the lease disappears, so a replacement address is probed.
    /// Milliseconds permit the protocol's 250 ms probe spacing.
    pub fn poll(&mut self, interface: &NetworkInterface, device: &mut impl FrameDevice, now: u64) {
        self.now = now;
        if self.address != interface.address() {
            self.address = interface.address();
            self.begin_probing(interface.mac_address());
        }
        if self.address == [0; 4] || now < self.deadline || self.suffix == u16::MAX {
            return;
        }
        let mut packet = [0; DNS_BUFFER_BYTES];
        let length = if self.probes < PROBE_COUNT {
            self.probe(&mut packet)
        } else if self.announcements < ANNOUNCEMENT_COUNT {
            self.answer(&mut packet, 0, false, TYPE_A)
        } else {
            return;
        };
        if send(
            interface,
            device,
            MULTICAST_MAC,
            MULTICAST_IP,
            PORT,
            &packet[..length],
        ) {
            if self.probes < PROBE_COUNT {
                self.probes += 1;
                self.deadline = now + PROBE_INTERVAL_MS;
            } else {
                self.announcements += 1;
                self.deadline = now + ANNOUNCEMENT_INTERVAL_MS;
            }
        } else {
            self.deadline = now + PROBE_INTERVAL_MS;
        }
    }

    fn begin_probing(&mut self, mac: [u8; 6]) {
        self.probes = 0;
        self.announcements = 0;
        // Vary the initial 0–250 ms delay across devices and boots.
        let seed = u16::from_be_bytes([mac[4], mac[5]]) as u64 ^ self.now;
        self.deadline = self.now + seed % (PROBE_INTERVAL_MS + 1);
    }

    /// Return true only for a packet addressed to this mDNS endpoint.
    pub fn handle(
        &mut self,
        interface: &NetworkInterface,
        device: &mut impl FrameDevice,
        frame: &[u8],
    ) -> bool {
        let Some(ethernet) = EthernetHeader::parse(frame) else {
            return false;
        };
        if ethernet.ethertype != ETHERTYPE_IPV4 {
            return false;
        }
        let bytes = &frame[ETHERNET_HEADER_LEN..];
        let Some(ip) = Ipv4Header::parse(bytes) else {
            return false;
        };
        let multicast = ip.dst_ip == MULTICAST_IP && ethernet.dst == MULTICAST_MAC;
        let unicast = ip.dst_ip == interface.address() && ethernet.dst == interface.mac_address();
        if ip.protocol != PROTOCOL_UDP || !(multicast || unicast) {
            return false;
        }
        let Some(datagram) = UdpDatagram::parse(
            &bytes[IPV4_HEADER_LEN..ip.total_len as usize],
            ip.src_ip,
            ip.dst_ip,
        ) else {
            return false;
        };
        if datagram.header.dst_port != PORT {
            return false;
        }
        if self.address == [0; 4]
            || self.address != interface.address()
            || ethernet.src == interface.mac_address()
        {
            return true;
        }
        let mut name = [0; 64];
        let name_length = self.name(&mut name);
        let Some(message) = inspect(datagram.payload, &name[..name_length], self.address) else {
            return true;
        };
        if message.response && (datagram.header.src_port != PORT || ip.ttl != LINK_LOCAL_HOP_LIMIT)
        {
            return true;
        }
        // During simultaneous probing, the lexicographically later IPv4 record
        // wins. An established responder's conflicting answer always wins.
        if message.conflicting_answer || (self.announcements == 0 && message.losing_probe) {
            self.suffix = self.suffix.saturating_add(1).max(2);
            self.begin_probing(interface.mac_address());
            return true;
        }
        if message.response
            || !message.question
            || self.probes < PROBE_COUNT
            || self.now < self.deadline && self.announcements == 0
        {
            return true;
        }
        let legacy = datagram.header.src_port != PORT;
        if message.known_answer && !legacy && !message.unicast {
            return true;
        }
        let mut packet = [0; DNS_BUFFER_BYTES];
        let length = self.answer(
            &mut packet,
            if legacy { message.id } else { 0 },
            legacy,
            message.question_type,
        );
        let (mac, address, port) = if legacy || message.unicast || unicast {
            (ethernet.src, ip.src_ip, datagram.header.src_port)
        } else {
            (MULTICAST_MAC, MULTICAST_IP, PORT)
        };
        send(interface, device, mac, address, port, &packet[..length]);
        true
    }

    fn name(&self, out: &mut [u8; 64]) -> usize {
        let base = b"gabix-pi2";
        out[1..1 + base.len()].copy_from_slice(base);
        let mut end = 1 + base.len();
        if self.suffix != 0 {
            out[end] = b'-';
            end += 1;
            // A fixed-width hexadecimal suffix keeps name construction bounded.
            for shift in [12, 8, 4, 0] {
                out[end] = b"0123456789abcdef"[((self.suffix >> shift) & 15) as usize];
                end += 1;
            }
        }
        out[0] = (end - 1) as u8;
        out[end..end + 7].copy_from_slice(b"\x05local\0");
        end + 7
    }

    fn question(&self, out: &mut [u8], kind: u16) -> usize {
        let mut name = [0; 64];
        let length = self.name(&mut name);
        out[..length].copy_from_slice(&name[..length]);
        out[length..length + 2].copy_from_slice(&kind.to_be_bytes());
        out[length + 2..length + 4].copy_from_slice(&CLASS_IN.to_be_bytes());
        length + 4
    }

    fn record(&self, out: &mut [u8], flush: bool, ttl: u32) -> usize {
        let end = self.question(out, TYPE_A);
        let class = CLASS_IN | if flush { UNICAST_OR_CACHE_FLUSH } else { 0 };
        out[end - 2..end].copy_from_slice(&class.to_be_bytes());
        out[end..end + 4].copy_from_slice(&ttl.to_be_bytes());
        out[end + 4..end + 6].copy_from_slice(&4u16.to_be_bytes());
        out[end + 6..end + 10].copy_from_slice(&self.address);
        end + 10
    }

    fn probe(&self, out: &mut [u8; DNS_BUFFER_BYTES]) -> usize {
        out[5] = 1; // One question; proposed record goes in the authority section.
        out[9] = 1;
        let end = DNS_HEADER_BYTES + self.question(&mut out[DNS_HEADER_BYTES..], TYPE_ANY);
        end + self.record(&mut out[end..], false, RECORD_TTL_SECONDS)
    }

    fn answer(
        &self,
        out: &mut [u8; DNS_BUFFER_BYTES],
        id: u16,
        legacy: bool,
        question_type: u16,
    ) -> usize {
        out[..2].copy_from_slice(&id.to_be_bytes());
        out[2..4].copy_from_slice(&(RESPONSE | AUTHORITATIVE).to_be_bytes());
        out[7] = 1;
        let mut end = DNS_HEADER_BYTES;
        if legacy {
            out[5] = 1;
            end += self.question(&mut out[end..], question_type);
        }
        end + self.record(
            &mut out[end..],
            !legacy,
            if legacy {
                LEGACY_TTL_SECONDS
            } else {
                RECORD_TTL_SECONDS
            },
        )
    }
}

fn send(
    interface: &NetworkInterface,
    device: &mut impl FrameDevice,
    mac: [u8; 6],
    ip: [u8; 4],
    port: u16,
    payload: &[u8],
) -> bool {
    let mut frame = [0; MAX_FRAME_LEN];
    let Some(length) = interface.write_udp(mac, ip, PORT, port, payload, &mut frame) else {
        return false;
    };
    // RFC 6762 requires outgoing IP TTL 255. Recompute only the IPv4 header
    // checksum; the UDP pseudo-header does not contain TTL.
    let header = &mut frame[ETHERNET_HEADER_LEN..ETHERNET_HEADER_LEN + IPV4_HEADER_LEN];
    header[8] = LINK_LOCAL_HOP_LIMIT;
    header[10..12].fill(0);
    let sum = checksum::internet_checksum(header);
    header[10..12].copy_from_slice(&sum.to_be_bytes());
    device.transmit(&frame[..length])
}

struct Message {
    id: u16,
    response: bool,
    question: bool,
    question_type: u16,
    unicast: bool,
    known_answer: bool,
    conflicting_answer: bool,
    losing_probe: bool,
}

fn word(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

/// Decode compressed names with a bounded walk. Pointers must go backwards,
/// so cyclic pointers and forward references cannot keep the parser running.
fn name_matches(bytes: &[u8], cursor: &mut usize, expected: &[u8]) -> Option<bool> {
    let mut at = *cursor;
    let mut consumed = None;
    let mut name = [0; 255];
    let mut end = 0;
    for _ in 0..255 {
        let length = *bytes.get(at)? as usize;
        if length & 0xc0 == 0xc0 {
            let target = (word(bytes, at)? & 0x3fff) as usize;
            if target < DNS_HEADER_BYTES || target >= at {
                return None;
            }
            consumed.get_or_insert(at + 2);
            at = target;
        } else {
            if length > 63 || end + length + 1 > name.len() {
                return None;
            }
            name[end..end + length + 1].copy_from_slice(bytes.get(at..at + length + 1)?);
            end += length + 1;
            at += length + 1;
            if length == 0 {
                *cursor = consumed.unwrap_or(at);
                return Some(name[..end].eq_ignore_ascii_case(expected));
            }
        }
    }
    None
}

fn inspect(bytes: &[u8], name: &[u8], address: [u8; 4]) -> Option<Message> {
    let flags = word(bytes, 2)?;
    // Ignore unsupported opcodes, errors and multipart/truncated messages.
    if flags & 0x7a0f != 0 {
        return None;
    }
    let mut message = Message {
        id: word(bytes, 0)?,
        response: flags & RESPONSE != 0,
        question: false,
        question_type: TYPE_A,
        unicast: false,
        known_answer: false,
        conflicting_answer: false,
        losing_probe: false,
    };
    let questions = word(bytes, 4)?;
    let answers = word(bytes, 6)?;
    let authorities = word(bytes, 8)?;
    let additional = word(bytes, 10)?;
    if questions > 32 || u32::from(answers) + u32::from(authorities) + u32::from(additional) > 64 {
        return None;
    }
    let mut at = DNS_HEADER_BYTES;
    for _ in 0..questions {
        let matches = name_matches(bytes, &mut at, name)?;
        let kind = word(bytes, at)?;
        let class = word(bytes, at + 2)?;
        at += 4;
        if matches
            && matches!(kind, TYPE_A | TYPE_ANY)
            && class & !UNICAST_OR_CACHE_FLUSH == CLASS_IN
        {
            message.question = true;
            message.question_type = kind;
            message.unicast |= class & UNICAST_OR_CACHE_FLUSH != 0;
        }
    }
    for index in 0..u32::from(answers) + u32::from(authorities) + u32::from(additional) {
        let matches = name_matches(bytes, &mut at, name)?;
        let kind = word(bytes, at)?;
        let class = word(bytes, at + 2)? & !UNICAST_OR_CACHE_FLUSH;
        let ttl = u32::from_be_bytes(bytes.get(at + 4..at + 8)?.try_into().ok()?);
        let length = word(bytes, at + 8)? as usize;
        let data = bytes.get(at + 10..at + 10 + length)?;
        at += 10 + length;
        if matches && kind == TYPE_A && class == CLASS_IN && length == 4 {
            let other: [u8; 4] = data.try_into().ok()?;
            if message.response && ttl != 0 && other != address {
                message.conflicting_answer = true;
            }
            if !message.response
                && index < u32::from(answers)
                && other == address
                && ttl >= RECORD_TTL_SECONDS / 2
            {
                message.known_answer = true;
            }
            if !message.response
                && index >= u32::from(answers)
                && index < u32::from(answers) + u32::from(authorities)
                && other > address
            {
                message.losing_probe = true;
            }
        }
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::EthernetFrame;
    use alloc::vec::Vec;

    struct Device(Vec<Vec<u8>>);
    impl FrameDevice for Device {
        fn mac_address(&self) -> [u8; 6] {
            [2, 0, 0, 0, 0, 1]
        }
        fn transmit(&mut self, frame: &[u8]) -> bool {
            self.0.push(frame.to_vec());
            true
        }
        fn receive(&mut self) -> Option<EthernetFrame> {
            None
        }
    }
    fn ready() -> (Host, NetworkInterface, Device) {
        let mut host = Host::new();
        let interface = NetworkInterface::new([2, 0, 0, 0, 0, 1], [192, 168, 1, 42]);
        let mut device = Device(Vec::new());
        for now in [0, 250, 500, 750, 1000, 2000] {
            host.poll(&interface, &mut device, now);
        }
        (host, interface, device)
    }
    fn query(host: &Host, port: u16, unicast: bool) -> Vec<u8> {
        let mut packet = [0; DNS_BUFFER_BYTES];
        packet[..2].copy_from_slice(&123u16.to_be_bytes());
        packet[5] = 1;
        let end = DNS_HEADER_BYTES + host.question(&mut packet[DNS_HEADER_BYTES..], TYPE_A);
        if unicast {
            packet[end - 2] |= 0x80;
        }
        let peer = NetworkInterface::new([2, 0, 0, 0, 0, 2], [192, 168, 1, 9]);
        let mut frame = [0; MAX_FRAME_LEN];
        let length = peer
            .write_udp(
                MULTICAST_MAC,
                MULTICAST_IP,
                port,
                PORT,
                &packet[..end],
                &mut frame,
            )
            .unwrap();
        frame[..length].to_vec()
    }
    fn dns(frame: &[u8]) -> &[u8] {
        &frame[ETHERNET_HEADER_LEN + IPV4_HEADER_LEN + 8..]
    }

    #[test]
    fn test_discovery_probes_announces_and_answers() {
        let (mut host, interface, mut device) = ready();
        assert_eq!(device.0.len(), 5);
        assert_eq!(word(dns(&device.0[0]), 8), Some(1));
        for frame in &device.0 {
            assert_eq!(
                Ipv4Header::parse(&frame[ETHERNET_HEADER_LEN..])
                    .unwrap()
                    .ttl,
                255
            );
        }
        let question = query(&host, PORT, false);
        assert!(host.handle(&interface, &mut device, &question));
        let response = device.0.last().unwrap();
        assert_eq!(&response[..6], &MULTICAST_MAC);
        assert_eq!(word(dns(response), 0), Some(0));
        assert_eq!(&response[response.len() - 4..], &interface.address());
        let mut replacement = NetworkInterface::new(interface.mac_address(), [0; 4]);
        host.poll(&replacement, &mut device, 3000);
        let count = device.0.len();
        host.handle(&replacement, &mut device, &question);
        assert_eq!(device.0.len(), count);
        replacement.configure([192, 168, 1, 43]);
        host.poll(&replacement, &mut device, 4000);
        host.poll(&replacement, &mut device, 4250);
        assert_eq!(word(dns(device.0.last().unwrap()), 8), Some(1));
    }

    #[test]
    fn test_queries_select_unicast_and_suppress_known_answers() {
        let (mut host, interface, mut device) = ready();
        for (port, qu) in [(PORT, true), (60000, false)] {
            host.handle(&interface, &mut device, &query(&host, port, qu));
            let frame = device.0.last().unwrap();
            assert_eq!(&frame[..6], &[2, 0, 0, 0, 0, 2]);
            assert_eq!(
                word(dns(frame), 0),
                Some(if port == PORT { 0 } else { 123 })
            );
            assert_eq!(word(dns(frame), 4), Some(if port == PORT { 0 } else { 1 }));
        }
        let peer = NetworkInterface::new([2, 0, 0, 0, 0, 2], [192, 168, 1, 9]);
        let mut incoming = Device(Vec::new());
        let mut packet = [0; DNS_BUFFER_BYTES];
        packet[5] = 1;
        packet[7] = 1;
        let question_end =
            DNS_HEADER_BYTES + host.question(&mut packet[DNS_HEADER_BYTES..], TYPE_A);
        let end =
            question_end + host.record(&mut packet[question_end..], false, RECORD_TTL_SECONDS);
        send(
            &peer,
            &mut incoming,
            MULTICAST_MAC,
            MULTICAST_IP,
            PORT,
            &packet[..end],
        );
        let count = device.0.len();
        host.handle(&interface, &mut device, incoming.0.last().unwrap());
        assert_eq!(
            device.0.len(),
            count,
            "A fresh known answer suppresses the response"
        );
        packet[end - 1] = 99;
        packet[2..4].copy_from_slice(&RESPONSE.to_be_bytes());
        send(
            &peer,
            &mut incoming,
            MULTICAST_MAC,
            MULTICAST_IP,
            PORT,
            &packet[..end],
        );
        host.handle(&interface, &mut device, incoming.0.last().unwrap());
        assert_eq!(
            device.0.len(),
            count,
            "A conflicting response starts probing instead of answering"
        );
        host.poll(&interface, &mut device, 2250);
        let probe = dns(device.0.last().unwrap());
        assert!(probe
            .windows(b"gabix-pi2-0002".len())
            .any(|part| part == b"gabix-pi2-0002"));
        assert_eq!(word(probe, 8), Some(1));
    }

    #[test]
    fn test_dns_rejects_truncation_and_pointer_cycles() {
        let (host, _, _) = ready();
        let mut packet = [0; DNS_BUFFER_BYTES];
        let end = host.probe(&mut packet);
        let mut name = [0; 64];
        let n = host.name(&mut name);
        for length in 0..end {
            assert!(inspect(&packet[..length], &name[..n], host.address).is_none());
        }
        assert!(inspect(&packet[..end], &name[..n], host.address).is_some());
        packet[12..14].copy_from_slice(&[0xc0, 12]);
        assert!(inspect(&packet[..end], &name[..n], host.address).is_none());
        // A backwards pointer to the first question is valid for a second name.
        let mut compressed = [0; 128];
        compressed[12..12 + n].copy_from_slice(&name[..n]);
        compressed[12 + n..14 + n].copy_from_slice(&[0xc0, 12]);
        let mut cursor = 12 + n;
        assert_eq!(
            name_matches(&compressed, &mut cursor, &name[..n]),
            Some(true)
        );
        assert_eq!(cursor, 14 + n);
    }
}
