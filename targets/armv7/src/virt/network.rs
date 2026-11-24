//! QEMU virt network adapter with one transmit and one receive buffer.
//! Transmit waits for completion; receive checks once and returns if empty.
//! VirtIO 1.2 sections 2.7, 4.2 and 5.1 define the queues and packet header.
use core::arch::asm;
use gabix_kernel::device::{EthernetFrame, FrameDevice, MAX_ETHERNET_FRAME_LEN};

const MMIO_START: usize = 0x0a00_0000;
const MMIO_STRIDE: usize = 0x200;
const MMIO_TRANSPORTS: usize = 32;

const MAGIC: usize = 0x000;
const VERSION: usize = 0x004;
const DEVICE_ID: usize = 0x008;
const DEVICE_FEATURES: usize = 0x010;
const DEVICE_FEATURES_SEL: usize = 0x014;
const DRIVER_FEATURES: usize = 0x020;
const DRIVER_FEATURES_SEL: usize = 0x024;
const QUEUE_SEL: usize = 0x030;
const QUEUE_NUM_MAX: usize = 0x034;
const QUEUE_NUM: usize = 0x038;
const QUEUE_READY: usize = 0x044;
const QUEUE_NOTIFY: usize = 0x050;
const STATUS: usize = 0x070;
const QUEUE_DESC_LOW: usize = 0x080;
const QUEUE_DRIVER_LOW: usize = 0x090;
const QUEUE_DEVICE_LOW: usize = 0x0a0;
const DEVICE_CONFIG: usize = 0x100;

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const VIRTIO_NET_DEVICE: u32 = 1;
const VIRTIO_NET_F_MAC: u32 = 1 << 5;
const MODERN_MMIO_VERSION: u32 = 2;
const FEATURE_WORD_LOW: u32 = 0;
const FEATURE_WORD_HIGH: u32 = 1;
// VIRTIO_F_VERSION_1 is bit 32, exposed as bit 0 in feature word 1.
const FEATURE_VERSION_1_IN_HIGH_WORD: u32 = 1 << 0;
const DEVICE_RESET: u32 = 0;
const QUEUE_ENABLED: u32 = 1;
const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

const TX_QUEUE: u32 = 1;
const RX_QUEUE: u32 = 0;
const QUEUE_SIZE: usize = 8;
const DESC_NEXT: u16 = 1;
const DESC_DEVICE_WRITES: u16 = 2;
// Modern header: flags, segmentation/checksum fields, and buffer count.
const VIRTIO_NET_HEADER_BYTES: usize = 12;
const MIN_ETHERNET_BYTES_WITHOUT_FCS: usize = 60;
const RX_BUFFER_LEN: usize = 2048;
const TX_HEADER_DESCRIPTOR: usize = 0;
const TX_FRAME_DESCRIPTOR: usize = 1;
const RX_BUFFER_DESCRIPTOR: usize = 0;

#[derive(Clone, Copy)]
#[repr(C)]
struct Descriptor {
    address: u64,
    len: u32,
    flags: u16,
    next: u16,
}

const EMPTY_DESCRIPTOR: Descriptor = Descriptor {
    address: 0,
    len: 0,
    flags: 0,
    next: 0,
};

#[repr(C, align(4096))]
struct DescriptorTable([Descriptor; QUEUE_SIZE]);

#[repr(C, align(4096))]
struct AvailableRing {
    flags: u16,
    index: u16,
    ring: [u16; QUEUE_SIZE],
    used_event: u16,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct UsedElement {
    id: u32,
    len: u32,
}

const EMPTY_USED: UsedElement = UsedElement { id: 0, len: 0 };

#[repr(C, align(4096))]
struct UsedRing {
    flags: u16,
    index: u16,
    ring: [UsedElement; QUEUE_SIZE],
    available_event: u16,
}

#[repr(C, align(16))]
struct NetHeader([u8; VIRTIO_NET_HEADER_BYTES]);

#[repr(C, align(64))]
struct FrameBuffer([u8; MAX_ETHERNET_FRAME_LEN]);

#[repr(C, align(4096))]
struct ReceiveBuffer([u8; RX_BUFFER_LEN]);

static mut TX_DESCRIPTORS: DescriptorTable = DescriptorTable([EMPTY_DESCRIPTOR; QUEUE_SIZE]);
static mut TX_AVAILABLE: AvailableRing = AvailableRing {
    flags: 0,
    index: 0,
    ring: [0; QUEUE_SIZE],
    used_event: 0,
};
static mut TX_USED: UsedRing = UsedRing {
    flags: 0,
    index: 0,
    ring: [EMPTY_USED; QUEUE_SIZE],
    available_event: 0,
};
static mut TX_HEADER: NetHeader = NetHeader([0; VIRTIO_NET_HEADER_BYTES]);
static mut TX_FRAME: FrameBuffer = FrameBuffer([0; MAX_ETHERNET_FRAME_LEN]);
static mut RX_DESCRIPTORS: DescriptorTable = DescriptorTable([EMPTY_DESCRIPTOR; QUEUE_SIZE]);
static mut RX_AVAILABLE: AvailableRing = AvailableRing {
    flags: 0,
    index: 0,
    ring: [0; QUEUE_SIZE],
    used_event: 0,
};
static mut RX_USED: UsedRing = UsedRing {
    flags: 0,
    index: 0,
    ring: [EMPTY_USED; QUEUE_SIZE],
    available_event: 0,
};
static mut RX_BUFFER: ReceiveBuffer = ReceiveBuffer([0; RX_BUFFER_LEN]);

pub struct VirtioNet {
    base: usize,
    mac: [u8; 6],
    tx_last_used: u16,
    rx_last_used: u16,
}

impl VirtioNet {
    pub unsafe fn probe() -> Option<Self> {
        for index in 0..MMIO_TRANSPORTS {
            let base = MMIO_START + index * MMIO_STRIDE;
            if read32(base + MAGIC) == VIRTIO_MAGIC
                && read32(base + VERSION) == MODERN_MMIO_VERSION
                && read32(base + DEVICE_ID) == VIRTIO_NET_DEVICE
            {
                let mut device = Self {
                    base,
                    mac: [0; 6],
                    tx_last_used: 0,
                    rx_last_used: 0,
                };
                if device.initialize() {
                    return Some(device);
                }
            }
        }
        None
    }

    unsafe fn initialize(&mut self) -> bool {
        write32(self.base + STATUS, DEVICE_RESET);
        write32(self.base + STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

        write32(self.base + DEVICE_FEATURES_SEL, FEATURE_WORD_LOW);
        if read32(self.base + DEVICE_FEATURES) & VIRTIO_NET_F_MAC == 0 {
            return false;
        }
        write32(self.base + DEVICE_FEATURES_SEL, FEATURE_WORD_HIGH);
        if read32(self.base + DEVICE_FEATURES) & FEATURE_VERSION_1_IN_HIGH_WORD == 0 {
            return false;
        }
        write32(self.base + DRIVER_FEATURES_SEL, FEATURE_WORD_LOW);
        write32(self.base + DRIVER_FEATURES, VIRTIO_NET_F_MAC);
        write32(self.base + DRIVER_FEATURES_SEL, FEATURE_WORD_HIGH);
        write32(self.base + DRIVER_FEATURES, FEATURE_VERSION_1_IN_HIGH_WORD);

        let feature_status = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
        write32(self.base + STATUS, feature_status);
        if read32(self.base + STATUS) & STATUS_FEATURES_OK == 0 {
            return false;
        }

        for (index, byte) in self.mac.iter_mut().enumerate() {
            *byte = core::ptr::read_volatile((self.base + DEVICE_CONFIG + index) as *const u8);
        }

        // Offer one writable buffer: VirtIO header followed by Ethernet bytes.
        write32(self.base + QUEUE_SEL, RX_QUEUE);
        if read32(self.base + QUEUE_NUM_MAX) < QUEUE_SIZE as u32 {
            return false;
        }
        write32(self.base + QUEUE_NUM, QUEUE_SIZE as u32);
        core::ptr::write_bytes(core::ptr::addr_of_mut!(RX_DESCRIPTORS), 0, 1);
        core::ptr::write_bytes(core::ptr::addr_of_mut!(RX_AVAILABLE), 0, 1);
        core::ptr::write_bytes(core::ptr::addr_of_mut!(RX_USED), 0, 1);
        core::ptr::write_bytes(
            core::ptr::addr_of_mut!(RX_BUFFER.0).cast::<u8>(),
            0,
            RX_BUFFER_LEN,
        );
        let rx_descriptors = &mut *core::ptr::addr_of_mut!(RX_DESCRIPTORS);
        core::ptr::write_volatile(
            &mut rx_descriptors.0[RX_BUFFER_DESCRIPTOR],
            Descriptor {
                address: core::ptr::addr_of!(RX_BUFFER) as u64,
                len: RX_BUFFER_LEN as u32,
                flags: DESC_DEVICE_WRITES,
                next: 0,
            },
        );
        let rx_available = &mut *core::ptr::addr_of_mut!(RX_AVAILABLE);
        core::ptr::write_volatile(&mut rx_available.ring[0], RX_BUFFER_DESCRIPTOR as u16);
        core::ptr::write_volatile(&mut rx_available.index, 1);
        write_address(
            self.base + QUEUE_DESC_LOW,
            core::ptr::addr_of!(RX_DESCRIPTORS) as u64,
        );
        write_address(
            self.base + QUEUE_DRIVER_LOW,
            core::ptr::addr_of!(RX_AVAILABLE) as u64,
        );
        write_address(
            self.base + QUEUE_DEVICE_LOW,
            core::ptr::addr_of!(RX_USED) as u64,
        );
        write32(self.base + QUEUE_READY, QUEUE_ENABLED);

        write32(self.base + QUEUE_SEL, TX_QUEUE);
        if read32(self.base + QUEUE_NUM_MAX) < QUEUE_SIZE as u32 {
            return false;
        }
        write32(self.base + QUEUE_NUM, QUEUE_SIZE as u32);
        core::ptr::write_bytes(core::ptr::addr_of_mut!(TX_DESCRIPTORS), 0, 1);
        core::ptr::write_bytes(core::ptr::addr_of_mut!(TX_AVAILABLE), 0, 1);
        core::ptr::write_bytes(core::ptr::addr_of_mut!(TX_USED), 0, 1);
        write_address(
            self.base + QUEUE_DESC_LOW,
            core::ptr::addr_of!(TX_DESCRIPTORS) as u64,
        );
        write_address(
            self.base + QUEUE_DRIVER_LOW,
            core::ptr::addr_of!(TX_AVAILABLE) as u64,
        );
        write_address(
            self.base + QUEUE_DEVICE_LOW,
            core::ptr::addr_of!(TX_USED) as u64,
        );
        write32(self.base + QUEUE_READY, QUEUE_ENABLED);
        write32(self.base + STATUS, feature_status | STATUS_DRIVER_OK);
        order_device_memory();
        write32(self.base + QUEUE_NOTIFY, RX_QUEUE);
        true
    }
}

unsafe impl Send for VirtioNet {}

impl FrameDevice for VirtioNet {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    // Caller supplies checksums. This synchronous poll has no timeout.
    fn transmit(&mut self, frame: &[u8]) -> bool {
        assert!(frame.len() <= MAX_ETHERNET_FRAME_LEN);
        let frame_len = frame.len().max(MIN_ETHERNET_BYTES_WITHOUT_FCS);

        unsafe {
            // Zero header disables checksum/segmentation requests; zero frame pads short packets.
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!(TX_HEADER.0).cast::<u8>(),
                0,
                VIRTIO_NET_HEADER_BYTES,
            );
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!(TX_FRAME.0).cast::<u8>(),
                0,
                MAX_ETHERNET_FRAME_LEN,
            );
            for (offset, byte) in frame.iter().enumerate() {
                core::ptr::write_volatile(
                    core::ptr::addr_of_mut!(TX_FRAME.0).cast::<u8>().add(offset),
                    *byte,
                );
            }

            // Both transmit descriptors are device-readable: header -> frame.
            let descriptors = &mut *core::ptr::addr_of_mut!(TX_DESCRIPTORS);
            core::ptr::write_volatile(
                &mut descriptors.0[TX_HEADER_DESCRIPTOR],
                Descriptor {
                    address: core::ptr::addr_of!(TX_HEADER) as u64,
                    len: VIRTIO_NET_HEADER_BYTES as u32,
                    flags: DESC_NEXT,
                    next: TX_FRAME_DESCRIPTOR as u16,
                },
            );
            core::ptr::write_volatile(
                &mut descriptors.0[TX_FRAME_DESCRIPTOR],
                Descriptor {
                    address: core::ptr::addr_of!(TX_FRAME) as u64,
                    len: frame_len as u32,
                    flags: 0,
                    next: 0,
                },
            );

            // Publish the chain before its available index, then notify queue 1.
            let available = &mut *core::ptr::addr_of_mut!(TX_AVAILABLE);
            let index = core::ptr::read_volatile(&available.index);
            core::ptr::write_volatile(
                &mut available.ring[index as usize % QUEUE_SIZE],
                TX_HEADER_DESCRIPTOR as u16,
            );
            order_device_memory();
            core::ptr::write_volatile(&mut available.index, index.wrapping_add(1));
            order_device_memory();
            write32(self.base + QUEUE_NOTIFY, TX_QUEUE);

            // Keep the transmit buffer unchanged until the device returns it.
            let used = &*core::ptr::addr_of!(TX_USED);
            while core::ptr::read_volatile(&used.index) == self.tx_last_used {
                core::hint::spin_loop();
            }
            self.tx_last_used = self.tx_last_used.wrapping_add(1);
            order_device_memory();
        }
        true
    }

    fn receive(&mut self) -> Option<EthernetFrame> {
        unsafe {
            let used = &*core::ptr::addr_of!(RX_USED);
            if core::ptr::read_volatile(&used.index) == self.rx_last_used {
                return None;
            }

            let element =
                core::ptr::read_volatile(&used.ring[self.rx_last_used as usize % QUEUE_SIZE]);
            self.rx_last_used = self.rx_last_used.wrapping_add(1);
            order_device_memory();

            // The used length includes the VirtIO header; callers receive Ethernet only.
            let written = (element.len as usize).min(RX_BUFFER_LEN);
            let frame =
                if element.id == RX_BUFFER_DESCRIPTOR as u32 && written > VIRTIO_NET_HEADER_BYTES {
                    let length = (written - VIRTIO_NET_HEADER_BYTES).min(MAX_ETHERNET_FRAME_LEN);
                    let source = core::ptr::addr_of!(RX_BUFFER.0)
                        .cast::<u8>()
                        .add(VIRTIO_NET_HEADER_BYTES);
                    let mut bytes = [0u8; MAX_ETHERNET_FRAME_LEN];
                    for (offset, byte) in bytes.iter_mut().take(length).enumerate() {
                        *byte = core::ptr::read_volatile(source.add(offset));
                    }
                    Some(EthernetFrame::from_bytes(&bytes[..length]))
                } else {
                    None
                };

            // Copying above releases our need for the received bytes. Reoffer the buffer.
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!(RX_BUFFER.0).cast::<u8>(),
                0,
                RX_BUFFER_LEN,
            );
            let available = &mut *core::ptr::addr_of_mut!(RX_AVAILABLE);
            let index = core::ptr::read_volatile(&available.index);
            core::ptr::write_volatile(
                &mut available.ring[index as usize % QUEUE_SIZE],
                RX_BUFFER_DESCRIPTOR as u16,
            );
            order_device_memory();
            core::ptr::write_volatile(&mut available.index, index.wrapping_add(1));
            order_device_memory();
            write32(self.base + QUEUE_NOTIFY, RX_QUEUE);
            frame
        }
    }
}

unsafe fn read32(address: usize) -> u32 {
    core::ptr::read_volatile(address as *const u32)
}

unsafe fn write32(address: usize, value: u32) {
    core::ptr::write_volatile(address as *mut u32, value);
}

unsafe fn write_address(low_register: usize, address: u64) {
    write32(low_register, address as u32);
    write32(low_register + 4, (address >> 32) as u32);
}

fn order_device_memory() {
    unsafe {
        asm!("dmb", options(nostack, preserves_flags));
    }
}
