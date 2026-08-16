/// Sector-oriented storage used by filesystems.
pub trait BlockDevice: Send {
    fn read_sector(&mut self, sector: u64) -> [u8; 512];
}

pub const MAX_ETHERNET_FRAME_LEN: usize = 1514;

/// One complete Ethernet frame copied out of device-owned memory.
pub struct EthernetFrame {
    bytes: [u8; MAX_ETHERNET_FRAME_LEN],
    len: usize,
}

impl EthernetFrame {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(
            bytes.len() <= MAX_ETHERNET_FRAME_LEN,
            "Ethernet frame exceeds the device boundary"
        );
        let mut frame = Self {
            bytes: [0; MAX_ETHERNET_FRAME_LEN],
            len: bytes.len(),
        };
        frame.bytes[..bytes.len()].copy_from_slice(bytes);
        frame
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// Raw Ethernet frame transport implemented by a network driver.
pub trait FrameDevice: Send {
    fn mac_address(&self) -> [u8; 6];
    fn transmit(&mut self, frame: &[u8]) -> bool;
    fn receive(&mut self) -> Option<EthernetFrame>;
}
