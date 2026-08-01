//! Hardware-independent device contracts and concrete x86 drivers.

pub mod pci;
pub mod virtio;

pub use gabix_kernel::device::{BlockDevice, EthernetFrame, FrameDevice, MAX_ETHERNET_FRAME_LEN};
