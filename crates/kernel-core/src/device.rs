/// Sector-oriented storage used by filesystems.
pub trait BlockDevice: Send {
    /// Read one 512-byte logical sector by index, relative to this device/adapter.
    /// This interface cannot report read errors. Implementations currently differ:
    /// the Pi SD adapter returns zeros on failure; ARM VirtIO panics. Callers needing
    /// error information must use the concrete driver's fallible read operation.
    fn read_sector(&mut self, sector: u64) -> [u8; 512];
}

/// Errors reported by a writable block device. Status words retain the driver's
/// diagnostic information; callers must treat any error as an incomplete write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockError {
    AddressOutOfRange,
    Timeout,
    Controller(u32),
    Card(u32),
}

/// Optional write capability. Read-only devices can implement BlockDevice alone.
/// Success means the device completed this sector write. It does not promise
/// an atomic update across power loss; update protocols must provide recovery.
pub trait WritableBlockDevice: BlockDevice {
    fn write_sector(&mut self, sector: u64, data: &[u8; 512]) -> Result<(), BlockError>;
}

/// 14-byte Ethernet header plus 1500-byte payload; excludes the hardware FCS.
pub const MAX_ETHERNET_FRAME_LEN: usize = 1514;

/// Owned frame bytes copied out of device memory; protocol validity is unchecked.
pub struct EthernetFrame {
    bytes: [u8; MAX_ETHERNET_FRAME_LEN],
    len: usize,
}

impl EthernetFrame {
    /// Copy bytes without parsing them. Empty/short inputs are accepted; inputs
    /// over the fixed capacity panic. Protocol parsers perform their own checks.
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
    /// Return whether the driver completed its local submission successfully.
    /// This does not confirm that the remote computer received the frame.
    fn transmit(&mut self, frame: &[u8]) -> bool;
    /// Poll for one owned frame. None supplies no error detail: it may represent
    /// no available packet or a driver failure. Poll duration is driver-specific.
    fn receive(&mut self) -> Option<EthernetFrame>;
}
