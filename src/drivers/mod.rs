//! Hardware-independent device contracts and concrete drivers.

pub mod pci;
pub mod virtio;

/// Sector-oriented storage used by filesystems.
///
/// Device drivers own transport details. Filesystems consume this boundary and
/// remain independent of PCI, virtio, and board-specific storage controllers.
pub trait BlockDevice: Send {
    fn read_sector(&mut self, sector: u64) -> [u8; 512];
}

pub const MAX_ETHERNET_FRAME_LEN: usize = 1514;

/// One complete Ethernet frame copied out of a device-owned receive buffer.
pub struct EthernetFrame {
    bytes: [u8; MAX_ETHERNET_FRAME_LEN],
    len: usize,
}

impl EthernetFrame {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(
            bytes.len() <= MAX_ETHERNET_FRAME_LEN,
            "Ethernet frame length {} exceeds {}",
            bytes.len(),
            MAX_ETHERNET_FRAME_LEN
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
    fn transmit(&mut self, frame: &[u8]);
    fn receive(&mut self) -> Option<EthernetFrame>;
}
