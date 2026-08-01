//! Ethernet header parsing and building.

pub const ETHERNET_HEADER_LEN: usize = 14;
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EthernetHeader {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ethertype: u16,
}

impl EthernetHeader {
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < ETHERNET_HEADER_LEN {
            return None;
        }
        let mut dst = [0u8; 6];
        let mut src = [0u8; 6];
        dst.copy_from_slice(&buf[0..6]);
        src.copy_from_slice(&buf[6..12]);
        Some(Self {
            dst,
            src,
            ethertype: u16::from_be_bytes([buf[12], buf[13]]),
        })
    }

    pub fn to_bytes(&self) -> [u8; ETHERNET_HEADER_LEN] {
        let mut out = [0u8; ETHERNET_HEADER_LEN];
        out[0..6].copy_from_slice(&self.dst);
        out[6..12].copy_from_slice(&self.src);
        out[12..14].copy_from_slice(&self.ethertype.to_be_bytes());
        out
    }
}
