//! virtio device driver — Lesson 6: capability list parsing and device init.
//!
//! Lesson 6 step 6.1: walk the PCI capability list of the virtio-blk device,
//! parse the virtio-specific vendor capabilities, and print each region's
//! location (BAR, offset, length).
//!
//! The PCI capability list lives in the upper half of config space (offsets
//! 0x40-0xFF). Each capability is a small header (cap_id, next_ptr, length)
//! followed by capability-specific body. virtio uses vendor-specific caps
//! (cap_id = 0x09) with a sub-header that points at a memory region inside
//! one of the device's BARs.
//!
//! The list is walked by reading the "capabilities pointer" at offset 0x34
//! in the standard config header, then following next_ptr until it's 0.

extern crate alloc;

use crate::drivers::pci;
use crate::println;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

static FIRST_DISK_COMPLETION_REPORTED: AtomicBool = AtomicBool::new(false);

/// Offset of the capabilities pointer in PCI config space.
const CAPABILITIES_POINTER_OFFSET: u8 = 0x34;

/// Capability ID for "vendor specific" — virtio uses this type.
const CAP_VENDOR_SPECIFIC: u8 = 0x09;
const CAP_MSIX: u8 = 0x11;
const MSIX_ENABLE: u16 = 1 << 15;

/// virtio vendor-specific capability sub-types (the `cfg_type` field).
const VIRTIO_CAP_COMMON: u8 = 1;
const VIRTIO_CAP_NOTIFY: u8 = 2;
const VIRTIO_CAP_ISR: u8 = 3;
const VIRTIO_CAP_DEVICE_CONFIG: u8 = 4;
const VIRTIO_CAP_PCI_CONFIG: u8 = 5;

// ============================================================
// Common config region field offsets (from the virtio 1.2 spec)
// ============================================================
const COMMON_DEVICE_FEATURE_SELECT: u8 = 0x00;
const COMMON_DEVICE_FEATURE: u8 = 0x04;
const COMMON_DRIVER_FEATURE_SELECT: u8 = 0x08;
const COMMON_DRIVER_FEATURE: u8 = 0x0C;
const COMMON_NUM_QUEUES: u8 = 0x12;
const COMMON_DEVICE_STATUS: u8 = 0x14;
const COMMON_CONFIG_GENERATION: u8 = 0x15;
const COMMON_QUEUE_SELECT: u8 = 0x16;
const COMMON_QUEUE_SIZE: u8 = 0x18;
const COMMON_QUEUE_MSIX_VECTOR: u8 = 0x1A;
const COMMON_QUEUE_ENABLE: u8 = 0x1C;
const COMMON_QUEUE_NOTIFY_OFF: u8 = 0x1E;
const COMMON_QUEUE_DESC: u8 = 0x20;
const COMMON_QUEUE_AVAIL: u8 = 0x28;
const COMMON_QUEUE_USED: u8 = 0x30;

// ============================================================
// Device status byte bits (the handshake)
// ============================================================
const STATUS_ACKNOWLEDGE: u8 = 0x01;
const STATUS_DRIVER: u8 = 0x02;
const STATUS_DRIVER_OK: u8 = 0x04;
const STATUS_FEATURES_OK: u8 = 0x08;
const STATUS_DEVICE_NEEDS_RESET: u8 = 0x40;
const STATUS_FAILED: u8 = 0x80;

/// virtio feature bits, indexed into the 64-bit feature space.
/// Transport features start at bit 32; device-specific features are 0-31.
const VIRTIO_F_VERSION_1: u64 = 1 << 32;

/// virtio-net feature bit: device config contains a valid MAC address.
const VIRTIO_NET_F_MAC: u32 = 1 << 5;

/// RX buffers are large enough for a virtio-net header plus one Ethernet MTU.
const VIRTIO_NET_RX_BUFFER_LEN: u32 = 2048;

/// Initial receive-buffer supply for Lesson 20.
const VIRTIO_NET_RX_BUFFER_COUNT: usize = 8;

/// virtio-net prepends this fixed header to every TX/RX frame.
const VIRTIO_NET_HDR_LEN: usize = 12;

mod block;
mod network;
mod queue;
mod transport;

pub use block::VirtioBlk;
pub use network::VirtioNet;

use queue::VirtQueue;
use transport::{parse_msix_capability, parse_virtio_capabilities, MsixCap};
