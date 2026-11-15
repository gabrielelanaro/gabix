//! DWC2 USB host transfers use channel 0 and DMA buffers in RAM.
//! The CPU supplies the buffer address, packet count and endpoint, then polls
//! channel status until the controller releases the buffer. Global USB IRQ
//! signaling stays disabled; status-register bits still record transfer events.

use core::arch::asm;

const BASE: usize = 0x3f98_0000;
const SYSTEM_TIMER_LOW: *const u32 = 0x3f00_3004 as *const u32;

const GAHBCFG: usize = 0x008;
#[cfg(feature = "pi-hardware")]
const GUSBCFG: usize = 0x00c;
#[cfg(feature = "pi-hardware")]
const GRSTCTL: usize = 0x010;
const GINTSTS: usize = 0x014;
#[cfg(feature = "pi-hardware")]
const GRXFSIZ: usize = 0x024;
#[cfg(feature = "pi-hardware")]
const GNPTXFSIZ: usize = 0x028;
#[cfg(feature = "pi-hardware")]
const GHWCFG3: usize = 0x04c;
#[cfg(feature = "pi-hardware")]
const HPTXFSIZ: usize = 0x100;
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
const AHB_GLOBAL_INTERRUPT_ENABLE: u32 = 1;
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
const CHANNEL_DISABLE: u32 = 1 << 30;
const DEVICE_ADDRESS_SHIFT: u32 = 22;
const ENDPOINT_TYPE_CONTROL: u32 = 0 << 18;
const ENDPOINT_TYPE_BULK: u32 = 2 << 18;
const ENDPOINT_DIRECTION_IN: u32 = 1 << 15;
const ENDPOINT_NUMBER_SHIFT: u32 = 11;
const INTERRUPT_TRANSFER_COMPLETE: u32 = 1 << 0;
const INTERRUPT_CHANNEL_HALTED: u32 = 1 << 1;
const INTERRUPT_STALL: u32 = 1 << 3;
const INTERRUPT_NAK: u32 = 1 << 4;
const INTERRUPT_TRANSACTION_ERROR: u32 = 1 << 7;
const INTERRUPT_AHB_ERROR: u32 = 1 << 2;
const INTERRUPT_BABBLE_ERROR: u32 = 1 << 8;
const INTERRUPT_DATA_TOGGLE_ERROR: u32 = 1 << 10;
const INTERRUPT_ERRORS: u32 = INTERRUPT_AHB_ERROR
    | INTERRUPT_TRANSACTION_ERROR
    | INTERRUPT_BABBLE_ERROR
    | INTERRUPT_DATA_TOGGLE_ERROR;
const PID_DATA1: u32 = 2;
const PID_SETUP: u32 = 3;
const PID_SHIFT: u32 = 29;
const PACKET_COUNT_SHIFT: u32 = 19;
const TRANSFER_SIZE_MASK: u32 = 0x7ffff;
const TRANSFER_ATTEMPT_LIMIT: usize = 1000;
const POLL_LIMIT: usize = 1_000_000;

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    HostToDevice,
    DeviceToHost,
}

#[derive(Clone, Copy, PartialEq)]
enum RetryPolicy {
    // Control requests and outgoing bulk data may retry NAK/transaction errors.
    RetryTransient,
    // Incoming bulk polling returns when the device has no packet available.
    ReturnOnNak,
}
const DATA_CACHE_LINE_SIZE: usize = 64;

// USB setup-packet values, independent of the DWC2 register encodings above.
mod request {
    pub const STANDARD_DEVICE_OUT: u8 = 0x00;
    pub const STANDARD_DEVICE_IN: u8 = 0x80;
    pub const CLASS_HUB_IN: u8 = 0xa0;
    pub const CLASS_PORT_OUT: u8 = 0x23;
    pub const CLASS_PORT_IN: u8 = 0xa3;
    pub const GET_STATUS: u8 = 0;
    pub const CLEAR_FEATURE: u8 = 1;
    pub const SET_FEATURE: u8 = 3;
    pub const SET_ADDRESS: u8 = 5;
    pub const GET_DESCRIPTOR: u8 = 6;
    pub const SET_CONFIGURATION: u8 = 9;
    // GET_DESCRIPTOR puts the type in the high byte and descriptor index in
    // the low byte of wValue. We request index zero in each case.
    pub const DEVICE_DESCRIPTOR: u16 = 0x0100;
    pub const CONFIGURATION_DESCRIPTOR: u16 = 0x0200;
    pub const HUB_DESCRIPTOR: u16 = 0x2900;
}

mod hub_port {
    pub const FEATURE_RESET: u16 = 4;
    pub const FEATURE_POWER: u16 = 8;
    pub const FEATURE_RESET_CHANGE: u16 = 20;
    pub const STATUS_CONNECTED: u16 = 1;
    pub const STATUS_ENABLED: u16 = 2;
}

const HUB_DEVICE_CLASS: u8 = 9;
const DEFAULT_DEVICE_ADDRESS: u8 = 0;
const ROOT_DEVICE_ADDRESS: u8 = 1;
const MAX_DEVICE_ADDRESS: u8 = 127;
const INITIAL_CONTROL_PACKET_SIZE: u8 = 64;
const DEVICE_DESCRIPTOR_BYTES: usize = 18;
const DEVICE_DESCRIPTOR_TYPE: u8 = 1;
const CONFIGURATION_HEADER_BYTES: usize = 9;
const CONTROL_BUFFER_BYTES: usize = 256;
const SETUP_PACKET_BYTES: usize = 8;
const ADDRESS_SETTLE_US: u32 = 10_000;
const HUB_POWER_SETTLE_US: u32 = 100_000;
const HUB_RESET_SETTLE_US: u32 = 50_000;
const ENDPOINT_DESCRIPTOR_TYPE: u8 = 5;
const ENDPOINT_DESCRIPTOR_BYTES: usize = 7;
const ENDPOINT_TRANSFER_TYPE_MASK: u8 = 3;
const ENDPOINT_TRANSFER_BULK: u8 = 2;
const ENDPOINT_ADDRESS_IN: u8 = 0x80;
const ENDPOINT_ADDRESS_NUMBER_MASK: u8 = 0x0f;

#[repr(C, align(64))]
struct DmaBuffer([u8; CONTROL_BUFFER_BYTES]);

static mut SETUP: DmaBuffer = DmaBuffer([0; CONTROL_BUFFER_BYTES]);
static mut DATA: DmaBuffer = DmaBuffer([0; CONTROL_BUFFER_BYTES]);
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
    pub outputs: [u8; 3],
    pub output_count: usize,
    pub max_packet_size: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HubEnumerationError {
    NotHub = 1,
    SetConfiguration = 2,
    ReadDescriptor = 3,
    NoPorts = 4,
    PowerPort = 5,
    ReadPortStatus = 6,
    ResetPort = 7,
    ReadResetStatus = 8,
    NoConnectedChild = 9,
    NoEnabledChild = 10,
    EnumerateChild = 11,
}

pub struct Dwc2;

impl Dwc2 {
    /// Snapshot the most recent channel result for a failed device operation.
    pub fn channel_status(&self) -> [u32; 4] {
        unsafe {
            [
                read32(CHANNEL_BASE + HCINT),
                read32(CHANNEL_BASE + HCTSIZ),
                read32(CHANNEL_BASE + HCCHAR),
                read32(CHANNEL_BASE + HCDMA),
            ]
        }
    }

    pub unsafe fn initialize() -> Option<Self> {
        #[cfg(feature = "pi-hardware")]
        initialize_hardware()?;

        if read32(GINTSTS) & CURRENT_MODE_HOST == 0 {
            return None;
        }
        #[cfg(not(feature = "pi-hardware"))]
        {
            write32(
                GAHBCFG,
                (read32(GAHBCFG) | AHB_DMA_ENABLE) & !AHB_GLOBAL_INTERRUPT_ENABLE,
            );
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
        self.enumerate_device(ROOT_DEVICE_ADDRESS)
    }

    pub unsafe fn enumerate_hub_child(&mut self, hub: UsbDevice) -> Option<UsbDevice> {
        self.enumerate_hub_child_diagnostic(hub).ok()
    }

    pub unsafe fn enumerate_hub_child_diagnostic(
        &mut self,
        hub: UsbDevice,
    ) -> Result<UsbDevice, HubEnumerationError> {
        let mut first = None;
        self.visit_hub_children(hub, |_, device| {
            first = Some(device);
            false
        })?;
        first.ok_or(HubEnumerationError::EnumerateChild)
    }

    /// Enumerate directly attached hub children with distinct USB addresses.
    /// Return true from `visit` to inspect the next port, or false to stop.
    /// Call once after root enumeration; resetting ports interrupts their devices.
    pub unsafe fn visit_hub_children(
        &mut self,
        hub: UsbDevice,
        mut visit: impl FnMut(u8, UsbDevice) -> bool,
    ) -> Result<(), HubEnumerationError> {
        if hub.device_class != HUB_DEVICE_CLASS {
            return Err(HubEnumerationError::NotHub);
        }
        self.control_write_no_data(
            hub,
            request::STANDARD_DEVICE_OUT,
            request::SET_CONFIGURATION,
            1,
            0,
        )
        .ok_or(HubEnumerationError::SetConfiguration)?;
        let mut descriptor = [0u8; 9];
        self.control_read(
            hub,
            request::CLASS_HUB_IN,
            request::GET_DESCRIPTOR,
            request::HUB_DESCRIPTOR,
            0,
            &mut descriptor,
        )
        .ok_or(HubEnumerationError::ReadDescriptor)?;
        // bNbrPorts follows the descriptor length and type bytes.
        let ports = descriptor[2];
        if ports == 0 {
            return Err(HubEnumerationError::NoPorts);
        }
        for port in 1..=ports {
            self.control_write_no_data(
                hub,
                request::CLASS_PORT_OUT,
                request::SET_FEATURE,
                hub_port::FEATURE_POWER,
                port as u16,
            )
            .ok_or(HubEnumerationError::PowerPort)?;
        }
        delay_us(HUB_POWER_SETTLE_US);

        let mut connected = false;
        let mut enabled = false;
        let mut found = false;
        for port in 1..=ports {
            let mut status = [0u8; 4];
            self.control_read(
                hub,
                request::CLASS_PORT_IN,
                request::GET_STATUS,
                0,
                port as u16,
                &mut status,
            )
            .ok_or(HubEnumerationError::ReadPortStatus)?;
            let port_status = u16::from_le_bytes([status[0], status[1]]);
            if port_status & hub_port::STATUS_CONNECTED == 0 {
                continue;
            }
            connected = true;
            self.control_write_no_data(
                hub,
                request::CLASS_PORT_OUT,
                request::SET_FEATURE,
                hub_port::FEATURE_RESET,
                port as u16,
            )
            .ok_or(HubEnumerationError::ResetPort)?;
            delay_us(HUB_RESET_SETTLE_US);
            self.control_read(
                hub,
                request::CLASS_PORT_IN,
                request::GET_STATUS,
                0,
                port as u16,
                &mut status,
            )
            .ok_or(HubEnumerationError::ReadResetStatus)?;
            let port_status = u16::from_le_bytes([status[0], status[1]]);
            if port_status & hub_port::STATUS_ENABLED == 0 {
                continue;
            }
            enabled = true;
            let _ = self.control_write_no_data(
                hub,
                request::CLASS_PORT_OUT,
                request::CLEAR_FEATURE,
                hub_port::FEATURE_RESET_CHANGE,
                port as u16,
            );
            // Address 1 belongs to the hub. Each child keeps its own address
            // while later ports are reset and enumerated at address zero.
            if port >= MAX_DEVICE_ADDRESS {
                break;
            }
            if let Some(device) = self.enumerate_device(port + 1) {
                found = true;
                if !visit(port, device) {
                    return Ok(());
                }
            }
        }
        if found {
            Ok(())
        } else if !connected {
            Err(HubEnumerationError::NoConnectedChild)
        } else if !enabled {
            Err(HubEnumerationError::NoEnabledChild)
        } else {
            Err(HubEnumerationError::EnumerateChild)
        }
    }

    unsafe fn enumerate_device(&mut self, new_address: u8) -> Option<UsbDevice> {
        // The first eight descriptor bytes include bMaxPacketSize0 at byte 7.
        // The device answers at address zero until SET_ADDRESS completes.
        let mut first = [0u8; 8];
        let first_len = self.control_in(
            DEFAULT_DEVICE_ADDRESS,
            INITIAL_CONTROL_PACKET_SIZE,
            SetupPacket::new(
                request::STANDARD_DEVICE_IN,
                request::GET_DESCRIPTOR,
                request::DEVICE_DESCRIPTOR,
                0,
                first.len() as u16,
            ),
            &mut first,
        )?;
        if first_len != first.len()
            || first[0] as usize != DEVICE_DESCRIPTOR_BYTES
            || first[1] != DEVICE_DESCRIPTOR_TYPE
        {
            return None;
        }
        let max_packet_size = first[7];
        if !matches!(max_packet_size, 8 | 16 | 32 | 64) {
            return None;
        }

        self.control_no_data(
            DEFAULT_DEVICE_ADDRESS,
            max_packet_size,
            SetupPacket::new(
                request::STANDARD_DEVICE_OUT,
                request::SET_ADDRESS,
                new_address as u16,
                0,
                0,
            ),
        )?;
        delay_us(ADDRESS_SETTLE_US);

        let mut descriptor = [0u8; DEVICE_DESCRIPTOR_BYTES];
        let length = self.control_in(
            new_address,
            max_packet_size,
            SetupPacket::new(
                request::STANDARD_DEVICE_IN,
                request::GET_DESCRIPTOR,
                request::DEVICE_DESCRIPTOR,
                0,
                descriptor.len() as u16,
            ),
            &mut descriptor,
        )?;
        if length != descriptor.len() || descriptor[1] != DEVICE_DESCRIPTOR_TYPE {
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
        let mut header = [0u8; CONFIGURATION_HEADER_BYTES];
        self.control_read(
            device,
            request::STANDARD_DEVICE_IN,
            request::GET_DESCRIPTOR,
            request::CONFIGURATION_DESCRIPTOR,
            0,
            &mut header,
        )?;
        // wTotalLength (bytes 2..4) includes all interface/endpoint descriptors;
        // bConfigurationValue (byte 5) is the value used to enable this setup.
        let total = u16::from_le_bytes([header[2], header[3]]) as usize;
        if total < header.len() || total > CONTROL_BUFFER_BYTES || header[5] == 0 {
            return None;
        }

        let mut descriptors = [0u8; CONTROL_BUFFER_BYTES];
        if self.control_read(
            device,
            request::STANDARD_DEVICE_IN,
            request::GET_DESCRIPTOR,
            request::CONFIGURATION_DESCRIPTOR,
            0,
            &mut descriptors[..total],
        )? != total
        {
            return None;
        }
        let endpoints = parse_bulk_endpoints(&descriptors[..total])?;
        // This LAN9512 revision may report a transaction error for the status
        // handshake after accepting SET_CONFIGURATION. The vendor-register
        // operations performed by its driver validate that it is configured.
        let _ = self.control_write_no_data(
            device,
            request::STANDARD_DEVICE_OUT,
            request::SET_CONFIGURATION,
            header[5] as u16,
            0,
        );
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
            Direction::HostToDevice,
            data_pid,
            bytes.as_ptr(),
            bytes.len(),
            RetryPolicy::RetryTransient,
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
            Direction::DeviceToHost,
            data_pid,
            bytes.as_mut_ptr(),
            bytes.len(),
            RetryPolicy::ReturnOnNak,
        )
    }

    unsafe fn control_in(
        &mut self,
        address: u8,
        max_packet_size: u8,
        setup: SetupPacket,
        out: &mut [u8],
    ) -> Option<usize> {
        if out.len() > CONTROL_BUFFER_BYTES {
            return None;
        }
        // SETUP stage: send the eight request bytes on endpoint zero.
        write_setup(setup);
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            Direction::HostToDevice,
            PID_SETUP,
            core::ptr::addr_of!(SETUP.0).cast::<u8>(),
            SETUP_PACKET_BYTES,
            RetryPolicy::RetryTransient,
        )?;

        core::ptr::write_bytes(
            core::ptr::addr_of_mut!(DATA.0).cast::<u8>(),
            0,
            CONTROL_BUFFER_BYTES,
        );
        // DATA stage: receive the requested bytes into the shared DMA buffer.
        let received = transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            Direction::DeviceToHost,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            // DWC2 IN DMA requires room for whole USB packets even when
            // wLength asks for a smaller register value. DATA has 256 bytes;
            // the validated EP0 packet sizes divide that capacity exactly.
            out.len().div_ceil(max_packet_size as usize) * max_packet_size as usize,
            RetryPolicy::RetryTransient,
        )?;
        if received > out.len() {
            return None;
        }
        for (offset, byte) in out.iter_mut().take(received).enumerate() {
            *byte = core::ptr::read_volatile(core::ptr::addr_of!(DATA.0).cast::<u8>().add(offset));
        }
        // STATUS stage: acknowledge the read with a zero-length OUT packet.
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            Direction::HostToDevice,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            0,
            RetryPolicy::RetryTransient,
        )?;
        Some(received)
    }

    unsafe fn control_no_data(
        &mut self,
        address: u8,
        max_packet_size: u8,
        setup: SetupPacket,
    ) -> Option<()> {
        // SETUP stage: send the eight request bytes on endpoint zero.
        write_setup(setup);
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            Direction::HostToDevice,
            PID_SETUP,
            core::ptr::addr_of!(SETUP.0).cast::<u8>(),
            SETUP_PACKET_BYTES,
            RetryPolicy::RetryTransient,
        )?;
        // This request has no DATA stage; receive its STATUS acknowledgement.
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            Direction::DeviceToHost,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            0,
            RetryPolicy::RetryTransient,
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
        if bytes.len() > CONTROL_BUFFER_BYTES {
            return None;
        }
        // SETUP stage: send the eight request bytes on endpoint zero.
        write_setup(setup);
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            Direction::HostToDevice,
            PID_SETUP,
            core::ptr::addr_of!(SETUP.0).cast::<u8>(),
            SETUP_PACKET_BYTES,
            RetryPolicy::RetryTransient,
        )?;
        // DATA stage: copy the payload into DMA storage, then send it.
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
            Direction::HostToDevice,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            bytes.len(),
            RetryPolicy::RetryTransient,
        )?;
        // STATUS stage: receive the device's zero-length acknowledgement.
        transfer(
            address,
            0,
            ENDPOINT_TYPE_CONTROL,
            max_packet_size as u16,
            Direction::DeviceToHost,
            PID_DATA1,
            core::ptr::addr_of!(DATA.0).cast::<u8>(),
            0,
            RetryPolicy::RetryTransient,
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
    direction: Direction,
    pid: u32,
    buffer: *const u8,
    length: usize,
    retry_policy: RetryPolicy,
) -> Option<usize> {
    let direction_in = direction == Direction::DeviceToHost;
    let retry_transient = retry_policy == RetryPolicy::RetryTransient;
    let packet_size = max_packet_size as usize;
    let packet_count = if length == 0 {
        1
    } else {
        length.div_ceil(packet_size)
    };
    for _ in 0..TRANSFER_ATTEMPT_LIMIT {
        prepare_dma_buffer(buffer, length, direction_in);
        // Clear the previous attempt's latched events by writing ones.
        write32(CHANNEL_BASE + HCINT, u32::MAX);
        write32(
            CHANNEL_BASE + HCINTMSK,
            INTERRUPT_TRANSFER_COMPLETE
                | INTERRUPT_CHANNEL_HALTED
                | INTERRUPT_NAK
                | INTERRUPT_STALL
                | INTERRUPT_ERRORS,
        );
        // HCDMA receives a bus address. The controller reads/writes RAM itself.
        write32(CHANNEL_BASE + HCDMA, dma_address(buffer));
        write32(
            CHANNEL_BASE + HCTSIZ,
            length as u32 | (packet_count as u32) << PACKET_COUNT_SHIFT | pid << PID_SHIFT,
        );
        let direction_bits = if direction_in {
            ENDPOINT_DIRECTION_IN
        } else {
            0
        };
        // Enabling the channel starts the configured transfer.
        write32(
            CHANNEL_BASE + HCCHAR,
            CHANNEL_ENABLE
                | (address as u32) << DEVICE_ADDRESS_SHIFT
                | endpoint_type
                | direction_bits
                | (endpoint as u32) << ENDPOINT_NUMBER_SHIFT
                | max_packet_size as u32,
        );
        if wait_until(|| {
            read32(CHANNEL_BASE + HCINT)
                & (INTERRUPT_TRANSFER_COMPLETE
                    | INTERRUPT_CHANNEL_HALTED
                    | (if retry_transient { 0 } else { INTERRUPT_NAK })
                    | INTERRUPT_STALL
                    | INTERRUPT_ERRORS)
                != 0
        })
        .is_none()
        {
            stop_channel_before_returning();
            complete_dma_buffer(buffer, length, direction_in);
            return None;
        }
        // 1. NAK means the device cannot supply or accept data yet.
        // In DMA mode the controller may keep retrying, so NAK alone does
        // not mean that it has stopped accessing our buffer.
        let status = read32(CHANNEL_BASE + HCINT);
        let device_not_ready = status & INTERRUPT_NAK != 0;
        if device_not_ready && !retry_transient && channel_is_enabled() {
            request_channel_halt();
        }

        // 2. Wait until the controller releases the channel before reusing
        // its registers or the memory buffer passed to this function.
        if wait_until(|| !channel_is_enabled()).is_none() {
            stop_channel_before_returning();
            complete_dma_buffer(buffer, length, direction_in);
            return None;
        }

        // 3. Read status again: the transfer may have completed while we
        // were requesting the halt. Keep a completed transfer's result.
        let status = read32(CHANNEL_BASE + HCINT);
        let transfer_completed = status & INTERRUPT_TRANSFER_COMPLETE != 0;
        let device_not_ready = status & INTERRUPT_NAK != 0;
        let transaction_failed = status & INTERRUPT_TRANSACTION_ERROR != 0;
        let transfer_failed = status & (INTERRUPT_STALL | INTERRUPT_ERRORS) != 0;
        complete_dma_buffer(buffer, length, direction_in);

        // 4. Retry recoverable conditions when this caller permits retries.
        if !transfer_completed && device_not_ready && retry_transient {
            continue;
        }
        if transaction_failed && retry_transient {
            continue;
        }
        if transfer_failed || !transfer_completed {
            return None;
        }
        // OUT completion confirms the whole submitted payload. DWC2 leaves
        // the OUT byte-count field unchanged; only IN reports residual bytes.
        if !direction_in {
            return Some(length);
        }
        let remaining = (read32(CHANNEL_BASE + HCTSIZ) & TRANSFER_SIZE_MASK) as usize;
        return Some(length.saturating_sub(remaining));
    }
    None
}

unsafe fn channel_is_enabled() -> bool {
    read32(CHANNEL_BASE + HCCHAR) & CHANNEL_ENABLE != 0
}

unsafe fn request_channel_halt() {
    let configuration = read32(CHANNEL_BASE + HCCHAR);
    // DWC2 requires BOTH bits to request a DMA-channel halt. Setting ENABLE
    // here submits the halt request; hardware clears it when the halt finishes.
    // Preserve the endpoint address, direction, and packet-size fields.
    write32(
        CHANNEL_BASE + HCCHAR,
        configuration | CHANNEL_DISABLE | CHANNEL_ENABLE,
    );
}

/// A timed-out transfer may still own the caller's memory. Returning while
/// DMA remains active would let hardware overwrite a reused stack buffer.
unsafe fn stop_channel_before_returning() {
    if channel_is_enabled() {
        request_channel_halt();
        if wait_until(|| !channel_is_enabled()).is_none() {
            // Keep the transfer's stack frame alive until watchdog recovery.
            panic!("USB channel did not halt; DMA buffer cannot be released");
        }
    }
}

unsafe fn prepare_dma_buffer(buffer: *const u8, length: usize, direction_in: bool) {
    #[cfg(feature = "pi-hardware")]
    if length != 0 {
        cache_lines(
            buffer,
            length,
            if direction_in {
                CacheOperation::CleanInvalidate
            } else {
                CacheOperation::Clean
            },
        );
    }
}

unsafe fn complete_dma_buffer(buffer: *const u8, length: usize, direction_in: bool) {
    #[cfg(feature = "pi-hardware")]
    if direction_in && length != 0 {
        cache_lines(buffer, length, CacheOperation::Invalidate);
    }
}

#[cfg(feature = "pi-hardware")]
#[derive(Clone, Copy)]
enum CacheOperation {
    Clean,
    Invalidate,
    CleanInvalidate,
}

#[cfg(feature = "pi-hardware")]
unsafe fn cache_lines(buffer: *const u8, length: usize, operation: CacheOperation) {
    let mut line = buffer as usize & !(DATA_CACHE_LINE_SIZE - 1);
    let end = (buffer as usize)
        .saturating_add(length)
        .saturating_add(DATA_CACHE_LINE_SIZE - 1)
        & !(DATA_CACHE_LINE_SIZE - 1);
    while line < end {
        match operation {
            CacheOperation::Clean => {
                asm!("mcr p15, 0, {address}, c7, c10, 1", address = in(reg) line as u32);
            }
            CacheOperation::Invalidate => {
                asm!("mcr p15, 0, {address}, c7, c6, 1", address = in(reg) line as u32);
            }
            CacheOperation::CleanInvalidate => {
                asm!("mcr p15, 0, {address}, c7, c14, 1", address = in(reg) line as u32);
            }
        }
        line += DATA_CACHE_LINE_SIZE;
    }
    asm!("dsb sy");
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

    let ahb_config = (read32(GAHBCFG) | AHB_DMA_ENABLE | AHB_WAIT_AXI_WRITES)
        & !AHB_MAX_BURST_MASK
        & !AHB_GLOBAL_INTERRUPT_ENABLE;
    write32(GAHBCFG, ahb_config);
    write32(PCGCCTL, 0);
    write32(HCFG, 0);

    // FIFO sizes and addresses are measured in 32-bit words. The reset
    // receive depth exceeds the BCM2836 controller's available FIFO RAM.
    // Give receive, non-periodic transmit, and periodic transmit distinct
    // 1024-word regions within its reported capacity.
    let fifo_words = read32(GHWCFG3) >> 16;
    if fifo_words < 3072 {
        return None;
    }
    write32(GRXFSIZ, 1024);
    write32(GNPTXFSIZ, (1024 << 16) | 1024);
    write32(HPTXFSIZ, (1024 << 16) | 2048);

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

// Busy-poll a condition for a bounded number of attempts. POLL_LIMIT counts
// register checks; it is not a duration and does not put the task to sleep.
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

// Walk length-prefixed descriptors. For each endpoint descriptor, byte 2 is
// its address, byte 3 its transfer type, and bytes 4..6 its maximum packet size.
// This parser retains the last IN/OUT endpoint and up to three OUT addresses;
// interface/alternate-setting selection is outside its current scope.
fn parse_bulk_endpoints(descriptors: &[u8]) -> Option<BulkEndpoints> {
    let mut input = None;
    let mut output = None;
    let mut outputs = [0; 3];
    let mut output_count = 0;
    let mut max_packet_size = 0;
    let mut offset = 0;
    while offset + 2 <= descriptors.len() {
        let length = descriptors[offset] as usize;
        if length < 2 || offset + length > descriptors.len() {
            return None;
        }
        if descriptors[offset + 1] == ENDPOINT_DESCRIPTOR_TYPE
            && length >= ENDPOINT_DESCRIPTOR_BYTES
            && descriptors[offset + 3] & ENDPOINT_TRANSFER_TYPE_MASK == ENDPOINT_TRANSFER_BULK
        {
            let endpoint = descriptors[offset + 2];
            max_packet_size =
                u16::from_le_bytes([descriptors[offset + 4], descriptors[offset + 5]]);
            if endpoint & ENDPOINT_ADDRESS_IN != 0 {
                input = Some(endpoint & ENDPOINT_ADDRESS_NUMBER_MASK);
            } else {
                output = Some(endpoint & ENDPOINT_ADDRESS_NUMBER_MASK);
                if output_count >= outputs.len() {
                    return None;
                }
                outputs[output_count] = endpoint & ENDPOINT_ADDRESS_NUMBER_MASK;
                output_count += 1;
            }
        }
        offset += length;
    }
    Some(BulkEndpoints {
        input: input?,
        output: output?,
        outputs,
        output_count,
        max_packet_size: max_packet_size.max(1),
    })
}
