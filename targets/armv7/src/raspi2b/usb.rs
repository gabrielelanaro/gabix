const BASE: usize = 0x3f98_0000;
const SYSTEM_TIMER_LOW: *const u32 = 0x3f00_3004 as *const u32;

const GAHBCFG: usize = 0x008;
const GINTSTS: usize = 0x014;
const HCFG: usize = 0x400;
const HPRT: usize = 0x440;
const CHANNEL_BASE: usize = 0x500;
const HCCHAR: usize = 0x00;
const HCINT: usize = 0x08;
const HCINTMSK: usize = 0x0c;
const HCTSIZ: usize = 0x10;
const HCDMA: usize = 0x14;

const AHB_DMA_ENABLE: u32 = 1 << 5;
const CURRENT_MODE_HOST: u32 = 1;
const PORT_POWER: u32 = 1 << 12;
const PORT_RESET: u32 = 1 << 8;
const PORT_ENABLE_CHANGE: u32 = 1 << 3;
const PORT_ENABLE: u32 = 1 << 2;
const PORT_CONNECT_DETECTED: u32 = 1 << 1;
const PORT_CONNECTED: u32 = 1 << 0;
const PORT_WRITE_TO_CLEAR: u32 = PORT_ENABLE_CHANGE | PORT_ENABLE | PORT_CONNECT_DETECTED;

const CHANNEL_ENABLE: u32 = 1 << 31;
const DEVICE_ADDRESS_SHIFT: u32 = 22;
const ENDPOINT_TYPE_CONTROL: u32 = 0 << 18;
const ENDPOINT_DIRECTION_IN: u32 = 1 << 15;
const INTERRUPT_TRANSFER_COMPLETE: u32 = 1 << 0;
const INTERRUPT_CHANNEL_HALTED: u32 = 1 << 1;
const INTERRUPT_STALL: u32 = 1 << 3;
const INTERRUPT_NAK: u32 = 1 << 4;
const INTERRUPT_ERRORS: u32 = (1 << 2) | (1 << 7) | (1 << 8) | (1 << 10);
const PID_DATA1: u32 = 2;
const PID_SETUP: u32 = 3;
const PID_SHIFT: u32 = 29;
const PACKET_COUNT_SHIFT: u32 = 19;
const POLL_LIMIT: usize = 1_000_000;

#[repr(C, align(64))]
struct DmaBuffer([u8; 256]);

static mut SETUP: DmaBuffer = DmaBuffer([0; 256]);
static mut DATA: DmaBuffer = DmaBuffer([0; 256]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsbDevice {
    pub address: u8,
    pub max_packet_size: u8,
    pub vendor_id: u16,
    pub product_id: u16,
}

pub struct Dwc2;

impl Dwc2 {
    pub unsafe fn initialize() -> Option<Self> {
        if read32(GINTSTS) & CURRENT_MODE_HOST == 0 {
            return None;
        }
        write32(GAHBCFG, read32(GAHBCFG) | AHB_DMA_ENABLE);
        write32(HCFG, 1);

        let port = port_value();
        if port & PORT_CONNECTED == 0 {
            return None;
        }
        write32(HPRT, port | PORT_POWER | PORT_RESET);
        delay_us(50_000);
        write32(HPRT, (port_value() & !PORT_RESET) | PORT_POWER);
        delay_us(20_000);
        wait_until(|| read32(HPRT) & PORT_ENABLE != 0)?;
        Some(Self)
    }

    pub unsafe fn enumerate_root(&mut self) -> Option<UsbDevice> {
        let mut first = [0u8; 8];
        let first_len = self.control_in(
            0,
            64,
            SetupPacket::new(0x80, 6, 0x0100, 0, first.len() as u16),
            &mut first,
        )?;
        if first_len != first.len() || first[0] != 18 || first[1] != 1 {
            return None;
        }
        let max_packet_size = first[7];
        if max_packet_size == 0 {
            return None;
        }

        self.control_no_data(0, max_packet_size, SetupPacket::new(0, 5, 1, 0, 0))?;
        delay_us(10_000);

        let mut descriptor = [0u8; 18];
        let length = self.control_in(
            1,
            max_packet_size,
            SetupPacket::new(0x80, 6, 0x0100, 0, descriptor.len() as u16),
            &mut descriptor,
        )?;
        if length != descriptor.len() || descriptor[1] != 1 {
            return None;
        }
        Some(UsbDevice {
            address: 1,
            max_packet_size,
            vendor_id: u16::from_le_bytes([descriptor[8], descriptor[9]]),
            product_id: u16::from_le_bytes([descriptor[10], descriptor[11]]),
        })
    }

    unsafe fn control_in(
        &mut self,
        address: u8,
        max_packet_size: u8,
        setup: SetupPacket,
        out: &mut [u8],
    ) -> Option<usize> {
        write_setup(setup);
        transfer(
            address,
            max_packet_size,
            false,
            PID_SETUP,
            core::ptr::addr_of!(SETUP.0).cast::<u8>(),
            8,
        )?;

        core::ptr::write_bytes(core::ptr::addr_of_mut!(DATA.0).cast::<u8>(), 0, 256);
        let received = transfer(
            address,
            max_packet_size,
            true,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            out.len(),
        )?;
        for (offset, byte) in out.iter_mut().take(received).enumerate() {
            *byte = core::ptr::read_volatile(core::ptr::addr_of!(DATA.0).cast::<u8>().add(offset));
        }
        transfer(
            address,
            max_packet_size,
            false,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            0,
        )?;
        Some(received)
    }

    unsafe fn control_no_data(
        &mut self,
        address: u8,
        max_packet_size: u8,
        setup: SetupPacket,
    ) -> Option<()> {
        write_setup(setup);
        transfer(
            address,
            max_packet_size,
            false,
            PID_SETUP,
            core::ptr::addr_of!(SETUP.0).cast::<u8>(),
            8,
        )?;
        transfer(
            address,
            max_packet_size,
            true,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            0,
        )?;
        Some(())
    }
}

#[derive(Clone, Copy)]
struct SetupPacket {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
}

impl SetupPacket {
    const fn new(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Self {
        Self {
            request_type,
            request,
            value,
            index,
            length,
        }
    }
}

unsafe fn write_setup(setup: SetupPacket) {
    let bytes = [
        setup.request_type,
        setup.request,
        setup.value as u8,
        (setup.value >> 8) as u8,
        setup.index as u8,
        (setup.index >> 8) as u8,
        setup.length as u8,
        (setup.length >> 8) as u8,
    ];
    for (offset, byte) in bytes.iter().enumerate() {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(SETUP.0).cast::<u8>().add(offset),
            *byte,
        );
    }
}

unsafe fn transfer(
    address: u8,
    max_packet_size: u8,
    direction_in: bool,
    pid: u32,
    buffer: *const u8,
    length: usize,
) -> Option<usize> {
    let packet_size = max_packet_size as usize;
    let packet_count = if length == 0 {
        1
    } else {
        length.div_ceil(packet_size)
    };
    for _ in 0..1000 {
        write32(CHANNEL_BASE + HCINT, u32::MAX);
        write32(
            CHANNEL_BASE + HCINTMSK,
            INTERRUPT_TRANSFER_COMPLETE
                | INTERRUPT_CHANNEL_HALTED
                | INTERRUPT_NAK
                | INTERRUPT_STALL
                | INTERRUPT_ERRORS,
        );
        write32(CHANNEL_BASE + HCDMA, buffer as u32);
        write32(
            CHANNEL_BASE + HCTSIZ,
            length as u32 | (packet_count as u32) << PACKET_COUNT_SHIFT | pid << PID_SHIFT,
        );
        let direction = if direction_in {
            ENDPOINT_DIRECTION_IN
        } else {
            0
        };
        write32(
            CHANNEL_BASE + HCCHAR,
            CHANNEL_ENABLE
                | (address as u32) << DEVICE_ADDRESS_SHIFT
                | ENDPOINT_TYPE_CONTROL
                | direction
                | max_packet_size as u32,
        );
        wait_until(|| {
            read32(CHANNEL_BASE + HCINT)
                & (INTERRUPT_TRANSFER_COMPLETE
                    | INTERRUPT_CHANNEL_HALTED
                    | INTERRUPT_NAK
                    | INTERRUPT_STALL
                    | INTERRUPT_ERRORS)
                != 0
        })?;
        let interrupt = read32(CHANNEL_BASE + HCINT);
        if interrupt & INTERRUPT_NAK != 0 {
            continue;
        }
        if interrupt & (INTERRUPT_STALL | INTERRUPT_ERRORS) != 0
            || interrupt & INTERRUPT_TRANSFER_COMPLETE == 0
        {
            return None;
        }
        let remaining = (read32(CHANNEL_BASE + HCTSIZ) & 0x7ffff) as usize;
        return Some(length.saturating_sub(remaining));
    }
    None
}

unsafe fn port_value() -> u32 {
    read32(HPRT) & !PORT_WRITE_TO_CLEAR
}

unsafe fn delay_us(microseconds: u32) {
    let start = core::ptr::read_volatile(SYSTEM_TIMER_LOW);
    while core::ptr::read_volatile(SYSTEM_TIMER_LOW).wrapping_sub(start) < microseconds {
        core::hint::spin_loop();
    }
}

unsafe fn wait_until(mut condition: impl FnMut() -> bool) -> Option<()> {
    for _ in 0..POLL_LIMIT {
        if condition() {
            return Some(());
        }
        core::hint::spin_loop();
    }
    None
}

unsafe fn read32(offset: usize) -> u32 {
    core::ptr::read_volatile((BASE + offset) as *const u32)
}

unsafe fn write32(offset: usize, value: u32) {
    core::ptr::write_volatile((BASE + offset) as *mut u32, value);
}
