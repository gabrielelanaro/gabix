//! QEMU virt block device using the modern VirtIO MMIO transport.
//! One synchronous read owns the shared request buffers until completion.
//! See VirtIO 1.2 sections 2.7 (split queues), 4.2 (MMIO), and 5.2 (block).
use core::arch::asm;
use gabix_kernel::device::BlockDevice;

// QEMU virt board address map: 32 transports, each occupying 512 bytes.
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

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const VIRTIO_BLOCK_DEVICE: u32 = 2;
const MODERN_MMIO_VERSION: u32 = 2;
const FEATURE_WORD_LOW: u32 = 0;
const FEATURE_WORD_HIGH: u32 = 1;
// VIRTIO_F_VERSION_1 is bit 32, exposed as bit 0 of feature word 1.
const FEATURE_VERSION_1_IN_HIGH_WORD: u32 = 1 << 0;
const DEVICE_RESET: u32 = 0;
const REQUEST_QUEUE: u32 = 0;
const QUEUE_ENABLED: u32 = 1;
const SECTOR_BYTES: usize = 512;
// Every request uses this chain: header -> sector data -> completion status.
const HEADER_DESCRIPTOR: usize = 0;
const DATA_DESCRIPTOR: usize = 1;
const STATUS_DESCRIPTOR: usize = 2;
const STATUS_ACKNOWLEDGE: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

const QUEUE_SIZE: usize = 8;
const DESC_NEXT: u16 = 1;
const DESC_DEVICE_WRITES: u16 = 2;
const REQUEST_READ_SECTOR: u32 = 0;
const REQUEST_STATUS_PENDING: u8 = 0xff;
const REQUEST_STATUS_OK: u8 = 0;

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
struct RequestHeader {
    request_type: u32,
    reserved: u32,
    sector: u64,
}

#[repr(C, align(512))]
struct Sector([u8; SECTOR_BYTES]);

static mut DESCRIPTORS: DescriptorTable = DescriptorTable([EMPTY_DESCRIPTOR; QUEUE_SIZE]);
static mut AVAILABLE: AvailableRing = AvailableRing {
    flags: 0,
    index: 0,
    ring: [0; QUEUE_SIZE],
    used_event: 0,
};
static mut USED: UsedRing = UsedRing {
    flags: 0,
    index: 0,
    ring: [EMPTY_USED; QUEUE_SIZE],
    available_event: 0,
};
static mut HEADER: RequestHeader = RequestHeader {
    request_type: REQUEST_READ_SECTOR,
    reserved: 0,
    sector: 0,
};
static mut DATA: Sector = Sector([0; SECTOR_BYTES]);
static mut REQUEST_STATUS: u8 = REQUEST_STATUS_PENDING;

pub struct VirtioBlock {
    base: usize,
    last_used: u16,
}

impl VirtioBlock {
    pub unsafe fn probe() -> Option<Self> {
        for index in 0..MMIO_TRANSPORTS {
            let base = MMIO_START + index * MMIO_STRIDE;
            if read32(base + MAGIC) == VIRTIO_MAGIC
                && read32(base + VERSION) == MODERN_MMIO_VERSION
                && read32(base + DEVICE_ID) == VIRTIO_BLOCK_DEVICE
            {
                let mut device = Self { base, last_used: 0 };
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

        write32(self.base + DEVICE_FEATURES_SEL, FEATURE_WORD_HIGH);
        if read32(self.base + DEVICE_FEATURES) & FEATURE_VERSION_1_IN_HIGH_WORD == 0 {
            return false;
        }
        write32(self.base + DRIVER_FEATURES_SEL, FEATURE_WORD_LOW);
        write32(self.base + DRIVER_FEATURES, 0);
        write32(self.base + DRIVER_FEATURES_SEL, FEATURE_WORD_HIGH);
        write32(self.base + DRIVER_FEATURES, FEATURE_VERSION_1_IN_HIGH_WORD);

        let feature_status = STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK;
        write32(self.base + STATUS, feature_status);
        if read32(self.base + STATUS) & STATUS_FEATURES_OK == 0 {
            return false;
        }

        write32(self.base + QUEUE_SEL, REQUEST_QUEUE);
        if read32(self.base + QUEUE_NUM_MAX) < QUEUE_SIZE as u32 {
            return false;
        }
        write32(self.base + QUEUE_NUM, QUEUE_SIZE as u32);

        core::ptr::write_bytes(core::ptr::addr_of_mut!(DESCRIPTORS), 0, 1);
        core::ptr::write_bytes(core::ptr::addr_of_mut!(AVAILABLE), 0, 1);
        core::ptr::write_bytes(core::ptr::addr_of_mut!(USED), 0, 1);

        write_address(
            self.base + QUEUE_DESC_LOW,
            core::ptr::addr_of!(DESCRIPTORS) as u64,
        );
        write_address(
            self.base + QUEUE_DRIVER_LOW,
            core::ptr::addr_of!(AVAILABLE) as u64,
        );
        write_address(
            self.base + QUEUE_DEVICE_LOW,
            core::ptr::addr_of!(USED) as u64,
        );
        write32(self.base + QUEUE_READY, QUEUE_ENABLED);
        write32(self.base + STATUS, feature_status | STATUS_DRIVER_OK);
        true
    }

    /// Polls the used ring until the device completes this read, with no timeout.
    /// The caller must serialize access to the module's shared request buffers.
    pub unsafe fn try_read_sector(&mut self, sector: u64) -> Option<[u8; SECTOR_BYTES]> {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(HEADER),
            RequestHeader {
                request_type: REQUEST_READ_SECTOR,
                reserved: 0,
                sector,
            },
        );
        core::ptr::write_bytes(
            core::ptr::addr_of_mut!(DATA.0).cast::<u8>(),
            0,
            SECTOR_BYTES,
        );
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(REQUEST_STATUS),
            REQUEST_STATUS_PENDING,
        );

        // The device reads our header, then writes the sector and status byte.
        let descriptors = &mut *core::ptr::addr_of_mut!(DESCRIPTORS);
        core::ptr::write_volatile(
            &mut descriptors.0[HEADER_DESCRIPTOR],
            Descriptor {
                address: core::ptr::addr_of!(HEADER) as u64,
                len: core::mem::size_of::<RequestHeader>() as u32,
                flags: DESC_NEXT,
                next: DATA_DESCRIPTOR as u16,
            },
        );
        core::ptr::write_volatile(
            &mut descriptors.0[DATA_DESCRIPTOR],
            Descriptor {
                address: core::ptr::addr_of!(DATA) as u64,
                len: SECTOR_BYTES as u32,
                flags: DESC_NEXT | DESC_DEVICE_WRITES,
                next: STATUS_DESCRIPTOR as u16,
            },
        );
        core::ptr::write_volatile(
            &mut descriptors.0[STATUS_DESCRIPTOR],
            Descriptor {
                address: core::ptr::addr_of!(REQUEST_STATUS) as u64,
                len: 1,
                flags: DESC_DEVICE_WRITES,
                next: 0,
            },
        );

        let available = &mut *core::ptr::addr_of_mut!(AVAILABLE);
        let available_index = core::ptr::read_volatile(&available.index);
        core::ptr::write_volatile(
            &mut available.ring[available_index as usize % QUEUE_SIZE],
            HEADER_DESCRIPTOR as u16,
        );
        // Publish the buffers and chain head before advancing the available index.
        order_device_memory();
        core::ptr::write_volatile(&mut available.index, available_index.wrapping_add(1));
        order_device_memory();
        write32(self.base + QUEUE_NOTIFY, REQUEST_QUEUE);

        // The device advances this index after writing the completed buffers.
        let used = &*core::ptr::addr_of!(USED);
        while core::ptr::read_volatile(&used.index) == self.last_used {
            core::hint::spin_loop();
        }
        self.last_used = self.last_used.wrapping_add(1);
        order_device_memory();

        if core::ptr::read_volatile(core::ptr::addr_of!(REQUEST_STATUS)) != REQUEST_STATUS_OK {
            return None;
        }

        let mut result = [0u8; SECTOR_BYTES];
        let data = core::ptr::addr_of!(DATA.0).cast::<u8>();
        for (offset, byte) in result.iter_mut().enumerate() {
            *byte = core::ptr::read_volatile(data.add(offset));
        }
        Some(result)
    }
}

unsafe impl Send for VirtioBlock {}

impl BlockDevice for VirtioBlock {
    fn read_sector(&mut self, sector: u64) -> [u8; SECTOR_BYTES] {
        unsafe {
            self.try_read_sector(sector)
                .expect("virtio block request failed")
        }
    }
}

unsafe fn read32(address: usize) -> u32 {
    core::ptr::read_volatile(address as *const u32)
}

unsafe fn write32(address: usize, value: u32) {
    core::ptr::write_volatile(address as *mut u32, value);
}

// MMIO splits each 64-bit queue address into adjacent low/high 32-bit words.
unsafe fn write_address(low_register: usize, address: u64) {
    write32(low_register, address as u32);
    write32(low_register + 4, (address >> 32) as u32);
}

fn order_device_memory() {
    unsafe {
        asm!("dmb", options(nostack, preserves_flags));
    }
}
