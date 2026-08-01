const BASE: usize = 0x3f98_0000;
const SYSTEM_TIMER_LOW: *const u32 = 0x3f00_3004 as *const u32;

const GAHBCFG: usize = 0x008;
#[cfg(feature = "pi-hardware")]
const GUSBCFG: usize = 0x00c;
#[cfg(feature = "pi-hardware")]
const GRSTCTL: usize = 0x010;
const GINTSTS: usize = 0x014;
const HCFG: usize = 0x400;
const HPRT: usize = 0x440;
const CHANNEL_BASE: usize = 0x500;
const HCCHAR: usize = 0x00;
const HCINT: usize = 0x08;
const HCINTMSK: usize = 0x0c;
const HCTSIZ: usize = 0x10;
const HCDMA: usize = 0x14;
#[cfg(feature = "pi-hardware")]
const PCGCCTL: usize = 0xe00;

const AHB_DMA_ENABLE: u32 = 1 << 5;
#[cfg(feature = "pi-hardware")]
const AHB_WAIT_AXI_WRITES: u32 = 1 << 4;
#[cfg(feature = "pi-hardware")]
const AHB_MAX_BURST_MASK: u32 = 3 << 1;
#[cfg(feature = "pi-hardware")]
const USB_PHY_INTERFACE_16_BIT: u32 = 1 << 3;
#[cfg(feature = "pi-hardware")]
const USB_ULPI_UTMI_SELECT: u32 = 1 << 4;
#[cfg(feature = "pi-hardware")]
const USB_ULPI_FSLS: u32 = 1 << 17;
#[cfg(feature = "pi-hardware")]
const USB_ULPI_CLOCK_SUSPEND: u32 = 1 << 19;
#[cfg(feature = "pi-hardware")]
const USB_ULPI_EXTERNAL_VBUS: u32 = 1 << 20;
#[cfg(feature = "pi-hardware")]
const USB_TERM_SELECT_PULSE: u32 = 1 << 22;
#[cfg(feature = "pi-hardware")]
const USB_FORCE_HOST_MODE: u32 = 1 << 29;
#[cfg(feature = "pi-hardware")]
const USB_FORCE_DEVICE_MODE: u32 = 1 << 30;
#[cfg(feature = "pi-hardware")]
const RESET_CORE_SOFT: u32 = 1 << 0;
#[cfg(feature = "pi-hardware")]
const RESET_RX_FIFO: u32 = 1 << 4;
#[cfg(feature = "pi-hardware")]
const RESET_TX_FIFO: u32 = 1 << 5;
#[cfg(feature = "pi-hardware")]
const RESET_TX_FIFO_ALL: u32 = 0x10 << 6;
#[cfg(feature = "pi-hardware")]
const RESET_AHB_IDLE: u32 = 1 << 31;
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
const ENDPOINT_TYPE_BULK: u32 = 2 << 18;
const ENDPOINT_DIRECTION_IN: u32 = 1 << 15;
const ENDPOINT_NUMBER_SHIFT: u32 = 11;
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
    pub device_class: u8,
    pub vendor_id: u16,
    pub product_id: u16,
}

pub struct BulkEndpoints {
    pub input: u8,
    pub output: u8,
    pub max_packet_size: u16,
}

pub struct Dwc2;

impl Dwc2 {
    pub unsafe fn initialize() -> Option<Self> {
        #[cfg(feature = "pi-hardware")]
        initialize_hardware()?;

        if read32(GINTSTS) & CURRENT_MODE_HOST == 0 {
            return None;
        }
        #[cfg(not(feature = "pi-hardware"))]
        {
            write32(GAHBCFG, read32(GAHBCFG) | AHB_DMA_ENABLE);
            write32(HCFG, 1);
        }

        write32(HPRT, port_value() | PORT_POWER);
        #[cfg(feature = "pi-hardware")]
        wait_for_us(1_000_000, || read32(HPRT) & PORT_CONNECTED != 0)?;
        #[cfg(not(feature = "pi-hardware"))]
        if port_value() & PORT_CONNECTED == 0 {
            return None;
        }
        delay_us(100_000);
        write32(HPRT, port_value() | PORT_POWER | PORT_RESET);
        delay_us(50_000);
        write32(HPRT, port_value() & !PORT_RESET | PORT_POWER);
        delay_us(20_000);
        wait_until(|| read32(HPRT) & PORT_ENABLE != 0)?;
        Some(Self)
    }

    pub unsafe fn enumerate_root(&mut self) -> Option<UsbDevice> {
        self.enumerate_device(1)
    }

    pub unsafe fn enumerate_hub_child(&mut self, hub: UsbDevice) -> Option<UsbDevice> {
        if hub.device_class != 9 {
            return None;
        }
        self.control_write_no_data(hub, 0, 9, 1, 0)?;
        let mut descriptor = [0u8; 9];
        self.control_read(hub, 0xa0, 6, 0x2900, 0, &mut descriptor)?;
        let ports = descriptor[2];
        if ports == 0 {
            return None;
        }
        for port in 1..=ports {
            self.control_write_no_data(hub, 0x23, 3, 8, port as u16)?;
        }
        delay_us(100_000);

        for port in 1..=ports {
            let mut status = [0u8; 4];
            self.control_read(hub, 0xa3, 0, 0, port as u16, &mut status)?;
            let port_status = u16::from_le_bytes([status[0], status[1]]);
            if port_status & 1 == 0 {
                continue;
            }
            self.control_write_no_data(hub, 0x23, 3, 4, port as u16)?;
            delay_us(50_000);
            self.control_read(hub, 0xa3, 0, 0, port as u16, &mut status)?;
            let port_status = u16::from_le_bytes([status[0], status[1]]);
            if port_status & 2 == 0 {
                continue;
            }
            let _ = self.control_write_no_data(hub, 0x23, 1, 20, port as u16);
            if let Some(device) = self.enumerate_device(2) {
                return Some(device);
            }
        }
        None
    }

    unsafe fn enumerate_device(&mut self, new_address: u8) -> Option<UsbDevice> {
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

        self.control_no_data(
            0,
            max_packet_size,
            SetupPacket::new(0, 5, new_address as u16, 0, 0),
        )?;
        delay_us(10_000);

        let mut descriptor = [0u8; 18];
        let length = self.control_in(
            new_address,
            max_packet_size,
            SetupPacket::new(0x80, 6, 0x0100, 0, descriptor.len() as u16),
            &mut descriptor,
        )?;
        if length != descriptor.len() || descriptor[1] != 1 {
            return None;
        }
        Some(UsbDevice {
            address: new_address,
            max_packet_size,
            device_class: descriptor[4],
            vendor_id: u16::from_le_bytes([descriptor[8], descriptor[9]]),
            product_id: u16::from_le_bytes([descriptor[10], descriptor[11]]),
        })
    }

    pub unsafe fn control_read(
        &mut self,
        device: UsbDevice,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        out: &mut [u8],
    ) -> Option<usize> {
        self.control_in(
            device.address,
            device.max_packet_size,
            SetupPacket::new(request_type, request, value, index, out.len() as u16),
            out,
        )
    }

    pub unsafe fn control_write_no_data(
        &mut self,
        device: UsbDevice,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
    ) -> Option<()> {
        self.control_no_data(
            device.address,
            device.max_packet_size,
            SetupPacket::new(request_type, request, value, index, 0),
        )
    }

    pub unsafe fn control_write(
        &mut self,
        device: UsbDevice,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        bytes: &[u8],
    ) -> Option<()> {
        self.control_out(
            device.address,
            device.max_packet_size,
            SetupPacket::new(request_type, request, value, index, bytes.len() as u16),
            bytes,
        )
    }

    pub unsafe fn configure_bulk_device(&mut self, device: UsbDevice) -> Option<BulkEndpoints> {
        let mut header = [0u8; 9];
        self.control_read(device, 0x80, 6, 0x0200, 0, &mut header)?;
        let total = u16::from_le_bytes([header[2], header[3]]) as usize;
        if total < header.len() || total > 256 || header[5] == 0 {
            return None;
        }

        let mut descriptors = [0u8; 256];
        if self.control_read(device, 0x80, 6, 0x0200, 0, &mut descriptors[..total])? != total {
            return None;
        }
        let endpoints = parse_bulk_endpoints(&descriptors[..total])?;
        self.control_write_no_data(device, 0, 9, header[5] as u16, 0)?;
        Some(endpoints)
    }

    pub unsafe fn bulk_out(
        &mut self,
        device: UsbDevice,
        endpoint: u8,
        max_packet_size: u16,
        data_pid: u32,
        bytes: &[u8],
    ) -> Option<usize> {
        transfer(
            device.address,
            endpoint,
            ENDPOINT_TYPE_BULK,
            max_packet_size,
            false,
            data_pid,
            bytes.as_ptr(),
            bytes.len(),
            false,
        )
    }

    pub unsafe fn bulk_in(
        &mut self,
        device: UsbDevice,
        endpoint: u8,
        max_packet_size: u16,
        data_pid: u32,
        bytes: &mut [u8],
    ) -> Option<usize> {
        transfer(
            device.address,
            endpoint,
            ENDPOINT_TYPE_BULK,
            max_packet_size,
            true,
            data_pid,
            bytes.as_mut_ptr(),
            bytes.len(),
            false,
        )
    }

    unsafe fn control_in(
        &mut self,
        address: u8,
        max_packet_size: u8,
        setup: SetupPacket,
        out: &mut [u8],
    ) -> Option<usize> {
        if out.len() > 256 {
            return None;
        }
        write_setup(setup);
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            false,
            PID_SETUP,
            core::ptr::addr_of!(SETUP.0).cast::<u8>(),
            8,
            true,
        )?;

        core::ptr::write_bytes(core::ptr::addr_of_mut!(DATA.0).cast::<u8>(), 0, 256);
        let received = transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            true,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            out.len(),
            true,
        )?;
        for (offset, byte) in out.iter_mut().take(received).enumerate() {
            *byte = core::ptr::read_volatile(core::ptr::addr_of!(DATA.0).cast::<u8>().add(offset));
        }
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            false,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            0,
            true,
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
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            false,
            PID_SETUP,
            core::ptr::addr_of!(SETUP.0).cast::<u8>(),
            8,
            true,
        )?;
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            true,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            0,
            true,
        )?;
        Some(())
    }

    unsafe fn control_out(
        &mut self,
        address: u8,
        max_packet_size: u8,
        setup: SetupPacket,
        bytes: &[u8],
    ) -> Option<()> {
        if bytes.len() > 256 {
            return None;
        }
        write_setup(setup);
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            false,
            PID_SETUP,
            core::ptr::addr_of!(SETUP.0).cast::<u8>(),
            8,
            true,
        )?;
        for (offset, byte) in bytes.iter().enumerate() {
            core::ptr::write_volatile(
                core::ptr::addr_of_mut!(DATA.0).cast::<u8>().add(offset),
                *byte,
            );
        }
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            false,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            bytes.len(),
            true,
        )?;
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            true,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            0,
            true,
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
    endpoint: u8,
    endpoint_type: u32,
    max_packet_size: u16,
    direction_in: bool,
    pid: u32,
    buffer: *const u8,
    length: usize,
    retry_nak: bool,
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
        write32(CHANNEL_BASE + HCDMA, dma_address(buffer));
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
                | endpoint_type
                | direction
                | (endpoint as u32) << ENDPOINT_NUMBER_SHIFT
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
        if interrupt & INTERRUPT_NAK != 0 && retry_nak {
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

#[cfg(feature = "pi-hardware")]
unsafe fn initialize_hardware() -> Option<()> {
    super::mailbox::power_on_usb()?;

    let mut usb_config = read32(GUSBCFG);
    usb_config &= !(USB_ULPI_EXTERNAL_VBUS | USB_TERM_SELECT_PULSE);
    write32(GUSBCFG, usb_config);
    wait_until(|| read32(GRSTCTL) & RESET_AHB_IDLE != 0)?;
    write32(GRSTCTL, read32(GRSTCTL) | RESET_CORE_SOFT);
    wait_until(|| read32(GRSTCTL) & RESET_CORE_SOFT == 0)?;
    delay_us(100_000);

    usb_config = read32(GUSBCFG);
    usb_config &= !(USB_PHY_INTERFACE_16_BIT
        | USB_ULPI_UTMI_SELECT
        | USB_ULPI_FSLS
        | USB_ULPI_CLOCK_SUSPEND
        | USB_FORCE_DEVICE_MODE);
    usb_config |= USB_FORCE_HOST_MODE;
    write32(GUSBCFG, usb_config);
    delay_us(50_000);

    let ahb_config = (read32(GAHBCFG) | AHB_DMA_ENABLE | AHB_WAIT_AXI_WRITES) & !AHB_MAX_BURST_MASK;
    write32(GAHBCFG, ahb_config);
    write32(PCGCCTL, 0);
    write32(HCFG, 0);

    write32(GRSTCTL, RESET_TX_FIFO | RESET_TX_FIFO_ALL);
    wait_until(|| read32(GRSTCTL) & RESET_TX_FIFO == 0)?;
    delay_us(1);
    write32(GRSTCTL, RESET_RX_FIFO);
    wait_until(|| read32(GRSTCTL) & RESET_RX_FIFO == 0)?;
    delay_us(1);
    Some(())
}

#[cfg(feature = "pi-hardware")]
unsafe fn wait_for_us(microseconds: u32, mut condition: impl FnMut() -> bool) -> Option<()> {
    let start = core::ptr::read_volatile(SYSTEM_TIMER_LOW);
    while core::ptr::read_volatile(SYSTEM_TIMER_LOW).wrapping_sub(start) < microseconds {
        if condition() {
            return Some(());
        }
        core::hint::spin_loop();
    }
    None
}

fn dma_address(buffer: *const u8) -> u32 {
    #[cfg(feature = "pi-hardware")]
    {
        (buffer as u32 & 0x3fff_ffff) | 0xc000_0000
    }
    #[cfg(not(feature = "pi-hardware"))]
    {
        buffer as u32
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

fn parse_bulk_endpoints(descriptors: &[u8]) -> Option<BulkEndpoints> {
    let mut input = None;
    let mut output = None;
    let mut max_packet_size = 0;
    let mut offset = 0;
    while offset + 2 <= descriptors.len() {
        let length = descriptors[offset] as usize;
        if length < 2 || offset + length > descriptors.len() {
            return None;
        }
        if descriptors[offset + 1] == 5 && length >= 7 && descriptors[offset + 3] & 3 == 2 {
            let endpoint = descriptors[offset + 2];
            max_packet_size =
                u16::from_le_bytes([descriptors[offset + 4], descriptors[offset + 5]]);
            if endpoint & 0x80 != 0 {
                input = Some(endpoint & 0x0f);
            } else {
                output = Some(endpoint & 0x0f);
            }
        }
        offset += length;
    }
    Some(BulkEndpoints {
        input: input?,
        output: output?,
        max_packet_size: max_packet_size.max(1),
    })
}
