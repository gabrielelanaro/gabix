//! Address Resolution Protocol (ARP) packet parsing and building.
//!
//! For Ethernet/IPv4, ARP asks which MAC address owns an IP address. The
//! eight-byte prefix identifies hardware/protocol types, address lengths,
//! and request/reply operation. Four address fields follow in this order:
//! sender MAC (8..14), sender IP (14..18), target MAC (18..24), target IP (24..28).
//! Field abbreviations below match RFC 826: htype/ptype are hardware/protocol
//! types; hlen/plen are address lengths; sha/spa are sender hardware/protocol
//! addresses; tha/tpa are target hardware/protocol addresses.

pub const ARP_PACKET_LEN: usize = 28;
pub const HTYPE_ETHERNET: u16 = 1;
pub const PTYPE_IPV4: u16 = 0x0800;
pub const HLEN_ETHERNET: u8 = 6;
pub const PLEN_IPV4: u8 = 4;
pub const OPER_REQUEST: u16 = 1;
pub const OPER_REPLY: u16 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArpPacket {
    pub htype: u16,
    pub ptype: u16,
    pub hlen: u8,
    pub plen: u8,
    pub oper: u16,
    pub sha: [u8; 6],
    pub spa: [u8; 4],
    pub tha: [u8; 6],
    pub tpa: [u8; 4],
}

impl ArpPacket {
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < ARP_PACKET_LEN {
            return None;
        }
        let mut sha = [0u8; 6];
        let mut spa = [0u8; 4];
        let mut tha = [0u8; 6];
        let mut tpa = [0u8; 4];
        sha.copy_from_slice(&buf[8..14]);
        spa.copy_from_slice(&buf[14..18]);
        tha.copy_from_slice(&buf[18..24]);
        tpa.copy_from_slice(&buf[24..28]);
        Some(Self {
            htype: u16::from_be_bytes([buf[0], buf[1]]),
            ptype: u16::from_be_bytes([buf[2], buf[3]]),
            hlen: buf[4],
            plen: buf[5],
            oper: u16::from_be_bytes([buf[6], buf[7]]),
            sha,
            spa,
            tha,
            tpa,
        })
    }

    pub fn request(sender_mac: [u8; 6], sender_ip: [u8; 4], target_ip: [u8; 4]) -> Self {
        Self {
            htype: HTYPE_ETHERNET,
            ptype: PTYPE_IPV4,
            hlen: HLEN_ETHERNET,
            plen: PLEN_IPV4,
            oper: OPER_REQUEST,
            sha: sender_mac,
            spa: sender_ip,
            tha: [0; 6],
            tpa: target_ip,
        }
    }

    pub fn reply_to(request: &Self, our_mac: [u8; 6], our_ip: [u8; 4]) -> Self {
        Self {
            htype: HTYPE_ETHERNET,
            ptype: PTYPE_IPV4,
            hlen: HLEN_ETHERNET,
            plen: PLEN_IPV4,
            oper: OPER_REPLY,
            sha: our_mac,
            spa: our_ip,
            tha: request.sha,
            tpa: request.spa,
        }
    }

    pub fn to_bytes(&self) -> [u8; ARP_PACKET_LEN] {
        let mut out = [0u8; ARP_PACKET_LEN];
        out[0..2].copy_from_slice(&self.htype.to_be_bytes());
        out[2..4].copy_from_slice(&self.ptype.to_be_bytes());
        out[4] = self.hlen;
        out[5] = self.plen;
        out[6..8].copy_from_slice(&self.oper.to_be_bytes());
        out[8..14].copy_from_slice(&self.sha);
        out[14..18].copy_from_slice(&self.spa);
        out[18..24].copy_from_slice(&self.tha);
        out[24..28].copy_from_slice(&self.tpa);
        out
    }

    pub fn is_ipv4_over_ethernet(&self) -> bool {
        self.htype == HTYPE_ETHERNET
            && self.ptype == PTYPE_IPV4
            && self.hlen == HLEN_ETHERNET
            && self.plen == PLEN_IPV4
    }
}
