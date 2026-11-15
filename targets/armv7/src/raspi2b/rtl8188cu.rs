//! RTL8188CU USB register transport and hardware initialization.
//! Register sequences follow OpenBSD rtwn.c / if_urtwn.c (see notice below).
/*-
 * Copyright (c) 2010 Damien Bergamini <damien.bergamini@free.fr>
 * Copyright (c) 2014 Kevin Lo <kevlo@FreeBSD.org>
 * Copyright (c) 2015 Stefan Sperling <stsp@openbsd.org>
 * Copyright (c) 2016 Nathanial Sloss <nathanialsloss@yahoo.com.au>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

#[path = "rtl8188cu_calibration.rs"]
mod calibration;
#[path = "rtl8188cu_tables.rs"]
mod tables;

use super::usb::{BulkEndpoints, Dwc2, UsbDevice};

// These are addresses inside the USB dongle. read*/write* access them through
// USB control messages; they are separate from the Pi USB-controller registers.
// Reference names are retained to make OpenBSD's r92creg.h easy to compare.
mod power {
    pub const STATE: u16 = 0x0004; // APS_FSMCO
    pub const AUTOLOAD_DONE: u16 = 1 << 1; // PFM_ALDN
    pub const REQUEST_MAC_POWER_ON: u16 = 1 << 8; // APFM_ONMAC
    pub const HARDWARE_SUSPEND: u16 = 1 << 11; // AFSM_HSUS
    pub const POWER_DOWN_ENABLED: u16 = 1 << 4; // PDN_EN
    pub const RUNNING_CONFIGURATION: u16 = HARDWARE_SUSPEND | POWER_DOWN_ENABLED | AUTOLOAD_DONE;

    pub const CONTROL_LOCK: u16 = 0x001c; // RSV_CTRL
    pub const UNLOCK_CONTROLS: u8 = 0;
    pub const SWITCHING_REGULATOR: u16 = 0x0011; // SPS0_CTRL
                                                 // Reference-driver setting for pulse-width modulation (PWM) mode.
    pub const REGULATOR_PWM_CONFIGURATION: u8 = 0x2b;
    pub const DIGITAL_REGULATOR: u16 = 0x0021; // LDOV12D_CTRL
    pub const DIGITAL_REGULATOR_ENABLED: u8 = 1; // LDV12_EN

    pub const ISOLATION: u16 = 0x0000; // SYS_ISO_CTRL
    pub const MD2PP_ISOLATION: u16 = 1; // MD2PP; reference does not expand the name.
    pub const RADIO_DIGITAL_ISOLATION: u16 = 1 << 9; // DIOR
    pub const MAC_SLEEP_CONTROL: u16 = 0x0600; // APSD_CTRL
    pub const MAC_OFF_REQUEST: u8 = 1 << 6; // OFF
    pub const MAC_OFF_STATUS: u16 = 1 << 7; // OFF_STATUS

    pub const BLOCK_ENABLE: u16 = 0x0100; // CR
    pub const USB_TRANSMIT_DMA: u16 = 1 << 0; // HCI_TXDMA_EN
    pub const USB_RECEIVE_DMA: u16 = 1 << 1; // HCI_RXDMA_EN
    pub const TRANSMIT_DMA: u16 = 1 << 2; // TXDMA_EN
    pub const RECEIVE_DMA: u16 = 1 << 3; // RXDMA_EN
    pub const PROTOCOL: u16 = 1 << 4; // PROTOCOL_EN
    pub const PACKET_SCHEDULER: u16 = 1 << 5; // SCHEDULE_EN
    pub const MAC_TRANSMIT: u16 = 1 << 6; // MACTXEN
    pub const MAC_RECEIVE: u16 = 1 << 7; // MACRXEN
    pub const SECURITY: u16 = 1 << 9; // ENSEC
    pub const REQUIRED_BLOCKS: u16 = USB_TRANSMIT_DMA
        | USB_RECEIVE_DMA
        | TRANSMIT_DMA
        | RECEIVE_DMA
        | PROTOCOL
        | PACKET_SCHEDULER
        | MAC_TRANSMIT
        | MAC_RECEIVE
        | SECURITY;

    // OpenBSD urtwn_r92c_power_on ends with this write but supplies no register
    // name or bit description. Preserve the tested value without inventing one.
    pub const UNDOCUMENTED_FINAL_REGISTER: u16 = 0xfe10;
    pub const UNDOCUMENTED_FINAL_VALUE: u8 = 0x19;

    pub const POLL_INTERVAL_US: u32 = 5;
    pub const MAC_POLL_INTERVAL_US: u32 = 50;
    pub const REGULATOR_SETTLE_US: u32 = 100;
}

// Warm-reset values from OpenBSD urtwn_r92c_power_off. Some are complete
// reference configurations rather than individual bits; retain their numeric
// values here so the ordered procedure below can state each operation's purpose.
mod shutdown {
    pub const TRANSMIT_PAUSE: u16 = 0x0522; // TXPAUSE
    pub const ALL_TRANSMIT_QUEUES: u8 = 0xff;
    pub const RADIO_POWER_CONTROL: u8 = 0; // RF_AC
    pub const RADIO_DISABLED: u32 = 0;
    pub const USB_ANALOG_AND_DIGITAL_ENABLED: u8 = 0x14; // SYS_FUNC_EN USBA | USBD
    pub const BASEBAND_GLOBAL_RESET: u8 = 1 << 1; // BB_GLB_RST
    pub const RESET_MAC_ENABLE_PROCESSOR: u8 = 0x54; // CPUEN | ELDR | HWPDN, high byte
    pub const SYSTEM_CLOCK: u16 = 0x0008; // SYS_CLKR
                                          // Includes the reference's unnamed bit 0x4000; do not infer its function.
    pub const MAC_CLOCK_DISABLED_CONFIGURATION: u16 = 0x70a3;
    pub const ANALOG_PLL: u16 = 0x0028; // AFE_PLL_CTRL
    pub const ANALOG_PLL_DISABLED: u8 = 0x80;
    pub const CRYSTAL_CONTROL: u16 = 0x0024; // AFE_XTAL_CTRL
    pub const ANALOG_DIGITAL_CLOCK_GATED: u16 = 0x880f;
    pub const DIGITAL_ISOLATION_CONFIGURATION: u8 = 0xf9;
    pub const LOADER_ISOLATION_CONFIGURATION: u8 = 0x82;

    pub const GPIO_LOW: u16 = 0x0044; // GPIO_PIN_CTRL
    pub const GPIO_LOW_OUTPUT_MASK: u32 = 0x0000_ff00;
    pub const GPIO_LOW_OUTPUT_SHIFT: u32 = 8;
    pub const GPIO_LOW_CONFIGURATION: u32 = 0x00ff_0000;
    pub const GPIO_PIN_MULTIPLEXER: u16 = 0x0043; // MAC_PINMUX_CFG
    pub const GPIO_HIGH: u16 = 0x0042; // GPIO_IO_SEL
    pub const GPIO_HIGH_INPUT_MASK: u16 = 0x000f;
    pub const GPIO_HIGH_OUTPUT_MASK: u16 = 0x00f0;
    pub const GPIO_HIGH_OUTPUT_SHIFT: u32 = 4;
    pub const GPIO_HIGH_CONFIGURATION: u16 = 0x0780;
    pub const LED_CONTROL: u16 = 0x004c; // LEDCFG0
    pub const BOTH_LEDS_DISABLED: u16 = 0x8080;

    pub const ANALOG_SUPPLY: u16 = 0x0020; // LDOA15_CTRL
    pub const ANALOG_SUPPLY_OFF_OUTPUT_BUFFER: u8 = 4; // OBUF without EN
    pub const REGULATOR_PFM_CONFIGURATION: u8 = 0x23;
    pub const USB_SUSPEND_CONFIGURATION: u16 = 0x4802; // APDM_HOST | AFSM_HSUS | PFM_ALDN
    pub const LOCK_POWER_CONTROLS: u8 = 0x0e; // RSV_CTRL
}

// Firmware is copied into the dongle's own processor memory. These constants
// describe its download window and host-to-firmware command registers.
// Reference: OpenBSD r92creg.h, rtwn_load_firmware, rtwn_fw_cmd.
mod fw {
    pub const CONTROL: u16 = 0x0080; // MCUFWDL
    pub const DOWNLOAD_ENABLED: u8 = 1 << 0;
    pub const DOWNLOAD_READY: u32 = 1 << 1;
    pub const CHECKSUM_REPORTED: u8 = 1 << 2;
    pub const INITIALIZATION_READY: u16 = 1 << 6; // WINTINI_RDY
    pub const RUNNING_FROM_RAM: u8 = 1 << 7; // RAM_DL_SEL
    pub const PAGE_MASK: u32 = 0x0007_0000;
    pub const PAGE_SHIFT: u32 = 16;
    pub const ROM_DOWNLOAD_ENABLED: u32 = 1 << 19; // ROM_DLEN
    pub const SYSTEM_FUNCTIONS: u16 = 0x0002; // SYS_FUNC_EN
    pub const PROCESSOR_ENABLED: u16 = 1 << 10; // CPUEN

    pub const HEADER_BYTES: usize = 32;
    pub const SIGNATURE_FAMILY_SHIFT: u32 = 4;
    pub const SIGNATURE_8188C: u16 = 0x88c;
    pub const SIGNATURE_8192C: u16 = 0x92c;
    pub const PAGE_BYTES: usize = 4096;
    pub const PAGE_COUNT: usize = 8;
    pub const WRITE_WINDOW: u16 = 0x1000;
    // The USB reference loader uses large blocks, then words, then bytes.
    pub const USB_BLOCK_BYTES: usize = 196;
    pub const USB_WORD_BYTES: usize = 4;

    pub const MAILBOX_STATUS: u16 = 0x01cc; // HMETFR
    pub const SELF_RESET_REGISTER: u16 = MAILBOX_STATUS + 3;
    pub const SELF_RESET_REQUEST: u8 = 0x20;
    pub const MAILBOX_BASE: u16 = 0x01d0; // HMEBOX
    pub const MAILBOX_EXTENSION_BASE: u16 = 0x0088; // HMEBOX_EXT
    pub const MAILBOX_COUNT: u8 = 4;
    pub const MAIN_WORD_BYTES: usize = 4;
    pub const EXTENSION_BYTES: usize = 2;
    pub const INLINE_PAYLOAD_BYTES: usize = MAIN_WORD_BYTES - 1;
    pub const MAX_COMMAND_PAYLOAD_BYTES: usize = INLINE_PAYLOAD_BYTES + EXTENSION_BYTES;
    pub const COMMAND_HAS_EXTENSION: u8 = 1 << 7;

    pub const RESET_ATTEMPTS: usize = 100;
    pub const RESET_POLL_US: u32 = 50;
    pub const DOWNLOAD_SETTLE_US: u32 = 50;
    pub const READBACK_RETRY_US: u32 = 100;
    pub const CHECKSUM_POLL_US: u32 = 5;
    pub const PROCESSOR_START_US: u32 = 5_000;
    pub const READY_POLL_US: u32 = 10;
    pub const MAILBOX_POLL_US: u32 = 10;
}

// Packet RAM is addressed in pages. LLT is the linked-list table that stores
// the next-page index; queue reservations and byte boundaries use other registers.
mod packet_memory {
    pub const LINK_COMMAND: u16 = 0x01e0;
    pub const LINK_WRITE: u32 = 1 << 30;
    pub const LINK_OPERATION_MASK: u32 = 3 << 30;
    pub const LINK_PAGE_SHIFT: u32 = 8;
    pub const PAGE_COUNT: u32 = 256;
    pub const LAST_TRANSMIT_PAGE: u32 = 248;
    pub const FIRST_RING_PAGE: u32 = 249;
    pub const LAST_RING_PAGE: u32 = PAGE_COUNT - 1;
    pub const END_OF_LIST: u32 = 0xff;
    pub const WRITE_ATTEMPTS: usize = 20;
    pub const WRITE_POLL_US: u32 = 5;
    pub const REQUIRED_OUT_PIPES: usize = 2;
    pub const NORMAL_QUEUE_PAGES_REGISTER: u16 = 0x0214;
    pub const QUEUE_PAGES_REGISTER: u16 = 0x0200;
    pub const NORMAL_QUEUE_PAGES: u8 = 2;
    pub const HIGH_QUEUE_PAGES: u32 = 12;
    pub const SHARED_QUEUE_PAGES: u32 = 234;
    pub const SHARED_PAGES_SHIFT: u32 = 16;
    pub const LOAD_QUEUE_PAGES: u32 = 1 << 31;
    // Beacon, management, loopback, TX FIFO and TX DMA boundary registers.
    pub const BOUNDARY_REGISTERS: [u16; 5] = [0x0424, 0x0425, 0x045d, 0x0114, 0x0209];
    pub const DMA_QUEUE_MAP: u16 = 0x010c;
    pub const QUEUE_MAP_MASK: u16 = 0xfff0;
    // Voice/video/management/high -> high pipe; best-effort/background -> normal.
    pub const HIGH_NORMAL_QUEUE_MAP: u16 = 0xfaf0;
    pub const RECEIVE_FIFO_BOUNDARY: u16 = 0x0116;
    pub const RECEIVE_LAST_BYTE: u16 = 0x27ff;
    pub const PAGE_SIZE_REGISTER: u16 = 0x0104;
    pub const PAGE_SIZE_128_BYTES: u8 = 1;
    pub const TRANSMIT_PAGE_SIZE_SHIFT: u32 = 4;
}

// Factory eFuse storage is a compressed stream of optional two-byte words.
// Decoding it produces the logical ROM map used by identity/calibration code.
mod efuse {
    pub const SYSTEM_CONFIG: u16 = 0x00f0;
    pub const TEST_CHIP: u32 = 1 << 23;
    pub const CHIP_8192C: u32 = 1 << 27;
    pub const READ_SUPPLY_ENABLED: u16 = 1 << 15;
    pub const EEPROM_LOADER_ENABLED: u16 = 1 << 12;
    pub const SYSTEM_CLOCK: u16 = 0x0008;
    pub const LOADER_CLOCK: u16 = 1 << 5;
    pub const ANALOG_8_MHZ_CLOCK: u16 = 1 << 1;
    pub const CONTROL: u16 = 0x0030;
    pub const READ_COMPLETE: u32 = 1 << 31;
    pub const ADDRESS_MASK: u32 = 0x0003ff00;
    pub const ADDRESS_SHIFT: u32 = 8;
    pub const POLL_ATTEMPTS: usize = 100;
    pub const POLL_DELAY_US: u32 = 5;
    pub const PHYSICAL_BYTES: u16 = 512;
    pub const LOGICAL_BYTES: usize = 128;
    pub const EMPTY: u8 = 0xff;
    pub const BLOCK_SHIFT: u32 = 4;
    pub const WORDS_PER_BLOCK: usize = 4;
    pub const WORD_BYTES: usize = 2;
    pub const BLOCK_BYTES: usize = WORDS_PER_BLOCK * WORD_BYTES;
    pub const ROM_SIGNATURE: [u8; 2] = [0x29, 0x81];
    pub const MAC_BYTES: core::ops::Range<usize> = 22..28;
    pub const PA_SETTING_ADDRESS: u16 = 0x1fa;
}

// CAM (content-addressable memory) holds the adapter's address/key entries.
// Write a data word, then issue a command with its word address and busy bit.
mod key_table {
    pub const COMMAND: u16 = 0x0670;
    pub const WRITE_DATA: u16 = 0x0674;
    pub const SECURITY_CONFIG: u16 = 0x0680;
    pub const BUSY: u32 = 1 << 31;
    pub const CLEAR_ALL: u32 = 1 << 30;
    pub const WRITE: u32 = 1 << 16;
    pub const WORDS_PER_ENTRY: u16 = 8;
    pub const KEY_FIRST_WORD: u16 = 2;
    pub const WORD_BYTES: usize = 4;
    pub const VALID: u32 = 1 << 15;
    pub const ALGORITHM_AES: u32 = 4 << 2;
    pub const MAC_LOW_SHIFT: u32 = 16;
    // Gabix uses entries 0..3 for group keys and entry 4 for the AP pairwise
    // key. The hardware has more entries; this bound is our station policy.
    pub const LAST_STATION_ENTRY: u8 = 4;
    pub const MAX_KEY_ID: u8 = 3;
    pub const POLL_ATTEMPTS: usize = 1000;
    pub const POLL_DELAY_US: u32 = 5;
    pub const TX_DEFAULT_UNICAST_KEY: u16 = 1 << 0;
    pub const RX_DEFAULT_UNICAST_KEY: u16 = 1 << 1;
    pub const TX_ENCRYPTION: u16 = 1 << 2;
    pub const RX_DECRYPTION: u16 = 1 << 3;
    pub const TX_DEFAULT_BROADCAST_KEY: u16 = 1 << 6;
    pub const RX_DEFAULT_BROADCAST_KEY: u16 = 1 << 7;
}

mod station_rate {
    pub const CONFIGURE_COMMAND: u8 = 6; // Firmware MACID_CONFIG command.
    pub const BROADCAST_ID: u8 = 4;
    pub const ACCESS_POINT_ID: u8 = 0;
    pub const ID_VALID: u8 = 0x80;
    pub const TABLE_80211B: u32 = 6 << 28;
    pub const CCK_RATE_MASK: u32 = 0x0f; // 1, 2, 5.5 and 11 Mbps.
    pub const INITIAL_RATE_BASE: u16 = 0x0484;
    pub const CCK1: u8 = 0;
}

// Adapter packet descriptors precede the 802.11 frame in USB bulk transfers.
// These fields belong to the dongle; DWC2's USB channel has a separate setup.
mod rx_descriptor {
    pub const BYTES: usize = 24;
    pub const PACKET_LENGTH_MASK: u32 = 0x3fff;
    pub const CRC_ERROR: u32 = 1 << 14;
    pub const INTEGRITY_CHECK_ERROR: u32 = 1 << 15;
    pub const DRIVER_INFO_SHIFT: u32 = 16;
    pub const DRIVER_INFO_MASK: u32 = 0xf;
    pub const DRIVER_INFO_UNIT_BYTES: usize = 8;
    pub const PADDING_SHIFT: u32 = 24;
    pub const PADDING_MASK: u32 = 3;
    pub const SOFTWARE_DECRYPTION_REQUIRED: u32 = 1 << 27;
}

mod tx_descriptor {
    pub const BYTES: usize = 32;
    pub const USB_BUFFER_BYTES: usize = 2048;
    pub const FRAME_OFFSET_SHIFT: u32 = 16;
    pub const MULTICAST: u32 = 1 << 24;
    pub const LAST_SEGMENT: u32 = 1 << 26;
    pub const FIRST_SEGMENT: u32 = 1 << 27;
    pub const OWNED_BY_ADAPTER: u32 = 1 << 31;
    pub const AGGREGATION_BREAK: u32 = 1 << 6;
    pub const BEST_EFFORT_QUEUE: u32 = 0 << 8;
    pub const MANAGEMENT_QUEUE: u32 = 0x12 << 8;
    pub const RATE_TABLE_80211B: u32 = 6 << 16;
    pub const CIPHER_AES: u32 = 3 << 22;
    pub const SEQUENCE_ENABLE: u16 = 1 << 15;
    pub const HARDWARE_SEQUENCE: u32 = 1 << 7;
    pub const USE_DRIVER_RATE: u32 = 1 << 8;
    pub const DATA_FALLBACK_LIMIT: u32 = 0x1f << 8;
    pub const RTS_FALLBACK_LIMIT: u32 = 0xf << 13;
    pub const CHECKSUM_BYTES: core::ops::Range<usize> = 28..30;
}

const WIFI_HEADER_BYTES: usize = 24;
const WIFI_FRAME_TYPE_MASK: u8 = 0x0c;
const WIFI_DATA_FRAME: u8 = 0x08;
const WIFI_PROTECTED_FRAME: u8 = 0x40;
// DWC2 encodes DATA0 as 0 and DATA1 as 2; XOR toggles the USB packet PID.
const USB_DATA_PID_TOGGLE: u32 = 2;

// Realtek vendor control requests: register address in wValue, zero wIndex,
// byte count in wLength. USB encodes both 16-bit fields least-significant first.
const USB_VENDOR_DEVICE_READ: u8 = 0xc0;
const USB_VENDOR_DEVICE_WRITE: u8 = 0x40;
const REALTEK_REGISTER_REQUEST: u8 = 5;
const UNUSED_REQUEST_INDEX: u16 = 0;
const REGISTER_POLL_ATTEMPTS: usize = 1000;

#[derive(Clone, Copy)]
enum BitState {
    Set,
    Clear,
}

#[derive(Debug, Clone, Copy)]
pub enum Error {
    WrongDevice,
    UsbRead(u16, Option<usize>, [u32; 4], &'static str),
    UsbWrite(u16),
    UsbTransmit([u32; 4]),
    Configuration,
    Calibration([[u16; 4]; 3]),
    UnsupportedChip(u32),
    Timeout {
        register: u16,
        mask: u16,
        expected: u16,
        observed: u16,
    },
    InvalidRom,
    InvalidFirmware,
    UnstableRegister {
        iteration: usize,
        expected: u32,
        actual: u32,
    },
    FirmwareReadback {
        page: u8,
        status: u32,
        address: u16,
        expected: u8,
        actual: u8,
        repeated: u8,
    },
}

pub struct ReceivedFrame<'a> {
    pub bytes: &'a [u8],
    pub hardware_decrypted: bool,
}

pub struct Identity {
    pub system_config: u32,
    pub mac: [u8; 6],
    pub rom: [u8; efuse::LOGICAL_BYTES],
    pub pa_setting: u8,
}

pub struct Rtl8188cu {
    usb: Dwc2,
    device: UsbDevice,
    pub endpoints: BulkEndpoints,
    pub rx_iq_calibrated: bool,
    rx_pid: u32,
    tx_pid: [u32; 3],
    channel_register: u32,
    firmware_mailbox: u8,
    phase: &'static str,
}

impl Rtl8188cu {
    /// The USB hub has already assigned this device an address.
    pub unsafe fn attach(mut usb: Dwc2, device: UsbDevice) -> Result<Self, Error> {
        if (device.vendor_id, device.product_id) != (0x0bda, 0x8176) {
            return Err(Error::WrongDevice);
        }
        let endpoints = usb
            .configure_bulk_device(device)
            .ok_or(Error::Configuration)?;
        Ok(Self {
            usb,
            device,
            endpoints,
            rx_iq_calibrated: false,
            rx_pid: 0,
            tx_pid: [0; 3],
            channel_register: 0,
            firmware_mailbox: 0,
            phase: "identity",
        })
    }

    /// Ask the dongle for N bytes at its register address. The USB driver sends
    /// an eight-byte SETUP request, then receives the register bytes into RAM.
    fn read<const N: usize>(&mut self, register: u16) -> Result<[u8; N], Error> {
        let mut bytes = [0; N];
        let received = unsafe {
            self.usb.control_read(
                self.device,
                USB_VENDOR_DEVICE_READ,
                REALTEK_REGISTER_REQUEST,
                register,
                UNUSED_REQUEST_INDEX,
                &mut bytes,
            )
        };
        if received != Some(N) {
            return Err(Error::UsbRead(
                register,
                received,
                self.usb.channel_status(),
                self.phase,
            ));
        }
        Ok(bytes)
    }

    /// Send a SETUP request naming the dongle register, followed by its new bytes.
    fn write(&mut self, register: u16, bytes: &[u8]) -> Result<(), Error> {
        unsafe {
            self.usb.control_write(
                self.device,
                USB_VENDOR_DEVICE_WRITE,
                REALTEK_REGISTER_REQUEST,
                register,
                UNUSED_REQUEST_INDEX,
                bytes,
            )
        }
        .ok_or(Error::UsbWrite(register))
    }

    fn read8(&mut self, register: u16) -> Result<u8, Error> {
        Ok(self.read::<1>(register)?[0])
    }
    fn read16(&mut self, register: u16) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.read(register)?))
    }
    fn read32(&mut self, register: u16) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.read(register)?))
    }
    fn write8(&mut self, register: u16, value: u8) -> Result<(), Error> {
        self.write(register, &[value])
    }
    fn write16(&mut self, register: u16, value: u16) -> Result<(), Error> {
        self.write(register, &value.to_le_bytes())
    }
    fn write32(&mut self, register: u16, value: u32) -> Result<(), Error> {
        self.write(register, &value.to_le_bytes())
    }
    fn update16(&mut self, register: u16, clear: u16, set: u16) -> Result<(), Error> {
        let value = self.read16(register)?;
        self.write16(register, (value & !clear) | set)
    }

    /// Set only the selected bits, preserving the register's other bits.
    fn set_bits16(&mut self, register: u16, bits: u16) -> Result<(), Error> {
        self.update16(register, 0, bits)
    }

    /// Clear only the selected bits, preserving the register's other bits.
    fn clear_bits16(&mut self, register: u16, bits: u16) -> Result<(), Error> {
        self.update16(register, bits, 0)
    }

    /// Read the factory-programmed identity and calibration bytes.
    /// These operations enable the eFuse reader; they never program a fuse.
    pub fn identify(&mut self) -> Result<Identity, Error> {
        let system_config = self.read32(efuse::SYSTEM_CONFIG)?;
        // This implementation supports the production 8188C, one radio chain.
        if system_config & (efuse::TEST_CHIP | efuse::CHIP_8192C) != 0 {
            return Err(Error::UnsupportedChip(system_config));
        }
        self.set_bits16(power::ISOLATION, efuse::READ_SUPPLY_ENABLED)?; // Enable eFuse read supply.
        self.set_bits16(fw::SYSTEM_FUNCTIONS, efuse::EEPROM_LOADER_ENABLED)?; // Release EEPROM loader reset.
        self.set_bits16(
            efuse::SYSTEM_CLOCK,
            efuse::LOADER_CLOCK | efuse::ANALOG_8_MHZ_CLOCK,
        )?; // Enable loader and 8 MHz clocks.
        let mut rom = [efuse::EMPTY; efuse::LOGICAL_BYTES];
        let mut address = 0u16;
        while address < efuse::PHYSICAL_BYTES {
            let header = self.efuse_byte(address)?;
            address += 1;
            if header == efuse::EMPTY {
                break;
            }
            // High nibble chooses an eight-byte logical block. Each low
            // nibble bit marks a word omitted from this record when set.
            let offset = (header >> efuse::BLOCK_SHIFT) as usize * efuse::BLOCK_BYTES;
            for word in 0..efuse::WORDS_PER_BLOCK {
                if header & (1 << word) != 0 {
                    continue;
                }
                if address + 1 >= efuse::PHYSICAL_BYTES {
                    return Err(Error::InvalidRom);
                }
                rom[offset + word * efuse::WORD_BYTES] = self.efuse_byte(address)?;
                rom[offset + word * efuse::WORD_BYTES + 1] = self.efuse_byte(address + 1)?;
                address += efuse::WORD_BYTES as u16;
            }
        }
        if rom[0..2] != efuse::ROM_SIGNATURE {
            return Err(Error::InvalidRom);
        }
        let mut mac = [0; 6];
        mac.copy_from_slice(&rom[efuse::MAC_BYTES]);
        if mac[0] & 1 != 0 || mac == [0; 6] {
            return Err(Error::InvalidRom);
        }
        Ok(Identity {
            system_config,
            mac,
            rom,
            pa_setting: self.efuse_byte(efuse::PA_SETTING_ADDRESS)?,
        })
    }

    fn efuse_byte(&mut self, address: u16) -> Result<u8, Error> {
        // Clear READ_COMPLETE while selecting the address to start a read.
        let value = self.read32(efuse::CONTROL)?;
        self.write32(
            efuse::CONTROL,
            (value & !(efuse::READ_COMPLETE | efuse::ADDRESS_MASK))
                | ((address as u32) << efuse::ADDRESS_SHIFT),
        )?;
        for _ in 0..efuse::POLL_ATTEMPTS {
            let value = self.read32(efuse::CONTROL)?;
            if value & efuse::READ_COMPLETE != 0 {
                return Ok(value as u8);
            }
            delay_us(efuse::POLL_DELAY_US);
        }
        // The shared timeout diagnostic reports a 16-bit register window.
        // Use the upper half here so it includes READ_COMPLETE.
        Err(Error::Timeout {
            register: efuse::CONTROL,
            mask: (efuse::READ_COMPLETE >> 16) as u16,
            expected: (efuse::READ_COMPLETE >> 16) as u16,
            observed: (self.read32(efuse::CONTROL)? >> 16) as u16,
        })
    }

    /// Return a warm adapter to the documented power-off state before init.
    /// A Pi reboot resets USB but can leave the dongle's 8051 and RF running.
    pub fn power_off(&mut self) -> Result<(), Error> {
        self.phase = "power off";

        // 1. Stop outgoing packets and disable the radio before resetting its
        // digital processing. USB stays enabled so we can send the next commands.
        self.write8(shutdown::TRANSMIT_PAUSE, shutdown::ALL_TRANSMIT_QUEUES)?;
        self.radio_write(shutdown::RADIO_POWER_CONTROL, shutdown::RADIO_DISABLED)?;
        self.write8(power::MAC_SLEEP_CONTROL, power::MAC_OFF_REQUEST)?;
        self.write8(
            fw::SYSTEM_FUNCTIONS,
            shutdown::USB_ANALOG_AND_DIGITAL_ENABLED | shutdown::BASEBAND_GLOBAL_RESET,
        )?;
        self.write8(
            fw::SYSTEM_FUNCTIONS,
            shutdown::USB_ANALOG_AND_DIGITAL_ENABLED,
        )?;

        // 2. Ask any running firmware to stop its processor. Preserve the
        // existing bounded wait and final explicit processor-disable write.
        if self.read8(fw::CONTROL)? & fw::DOWNLOAD_READY as u8 != 0 {
            self.write8(fw::CONTROL, 0)?;
            self.write8(fw::SELF_RESET_REGISTER, fw::SELF_RESET_REQUEST)?;
            for _ in 0..fw::RESET_ATTEMPTS {
                if self.read16(fw::SYSTEM_FUNCTIONS)? & fw::PROCESSOR_ENABLED == 0 {
                    break;
                }
                delay_us(fw::RESET_POLL_US);
            }
            self.clear_bits16(fw::SYSTEM_FUNCTIONS, fw::PROCESSOR_ENABLED)?;
        }

        // 3. Apply the reference digital reset, stop clocks, and isolate the
        // powered-down circuits. +1 deliberately addresses the high byte.
        self.write8(
            fw::SYSTEM_FUNCTIONS + 1,
            shutdown::RESET_MAC_ENABLE_PROCESSOR,
        )?;
        self.write8(fw::CONTROL, 0)?;
        self.write16(
            shutdown::SYSTEM_CLOCK,
            shutdown::MAC_CLOCK_DISABLED_CONFIGURATION,
        )?;
        self.write8(shutdown::ANALOG_PLL, shutdown::ANALOG_PLL_DISABLED)?;
        self.write16(
            shutdown::CRYSTAL_CONTROL,
            shutdown::ANALOG_DIGITAL_CLOCK_GATED,
        )?;
        self.write8(power::ISOLATION, shutdown::DIGITAL_ISOLATION_CONFIGURATION)?;

        // 4. Put the general-purpose I/O (GPIO) pins and LEDs into the reference
        // shutdown configuration. Copy sampled pin levels into their output
        // fields before enabling those outputs, as the reference sequence does.
        self.write16(shutdown::GPIO_LOW + 2, 0)?;
        let gpio = self.read32(shutdown::GPIO_LOW)? & !shutdown::GPIO_LOW_OUTPUT_MASK;
        self.write32(
            shutdown::GPIO_LOW,
            gpio | ((gpio << shutdown::GPIO_LOW_OUTPUT_SHIFT) & shutdown::GPIO_LOW_OUTPUT_MASK)
                | shutdown::GPIO_LOW_CONFIGURATION,
        )?;
        self.write8(shutdown::GPIO_PIN_MULTIPLEXER, 0)?;
        let gpio = self.read16(shutdown::GPIO_HIGH)? & !shutdown::GPIO_HIGH_OUTPUT_MASK;
        self.write16(
            shutdown::GPIO_HIGH,
            gpio | ((gpio & shutdown::GPIO_HIGH_INPUT_MASK) << shutdown::GPIO_HIGH_OUTPUT_SHIFT)
                | shutdown::GPIO_HIGH_CONFIGURATION,
        )?;
        self.write16(shutdown::LED_CONTROL, shutdown::BOTH_LEDS_DISABLED)?;

        // 5. Isolate the loader, switch off the analog/digital supplies, then
        // select pulse-frequency modulation (PFM) for the switching regulator.
        self.write16(
            shutdown::SYSTEM_CLOCK,
            shutdown::MAC_CLOCK_DISABLED_CONFIGURATION,
        )?;
        self.write8(
            power::ISOLATION + 1,
            shutdown::LOADER_ISOLATION_CONFIGURATION,
        )?;
        self.write8(
            shutdown::ANALOG_SUPPLY,
            shutdown::ANALOG_SUPPLY_OFF_OUTPUT_BUFFER,
        )?;
        let regulator = self.read8(power::DIGITAL_REGULATOR)?;
        self.write8(
            power::DIGITAL_REGULATOR,
            regulator & !power::DIGITAL_REGULATOR_ENABLED,
        )?;
        self.write8(
            power::SWITCHING_REGULATOR,
            shutdown::REGULATOR_PFM_CONFIGURATION,
        )?;

        // 6. Finish the USB suspend sequence and lock power/clock controls.
        self.write16(power::STATE, shutdown::USB_SUSPEND_CONFIGURATION)?;
        self.write8(power::CONTROL_LOCK, shutdown::LOCK_POWER_CONTROLS)
    }

    /// Enable the dongle's internal power supplies and packet-processing blocks.
    /// USB already works at this point. Firmware loading and Wi-Fi association
    /// happen later. Sequence: OpenBSD if_urtwn.c, urtwn_r92c_power_on.
    pub fn power_on(&mut self) -> Result<(), Error> {
        self.phase = "power on";

        // 1. Wait for the dongle to finish loading its initial configuration.
        // Each attempt reads two bytes over USB and checks AUTOLOAD_DONE.
        self.wait_for_bits16(
            power::STATE,
            power::AUTOLOAD_DONE,
            BitState::Set,
            power::POLL_INTERVAL_US,
        )?;

        // 2. Allow power/clock settings to be changed, then select the reference
        // switching-regulator mode. The delay lets that supply settle.
        self.write8(power::CONTROL_LOCK, power::UNLOCK_CONTROLS)?;
        self.write8(
            power::SWITCHING_REGULATOR,
            power::REGULATOR_PWM_CONFIGURATION,
        )?;
        delay_us(power::REGULATOR_SETTLE_US);

        // 3. Enable the digital supply if it is off. Preserve its other settings.
        // Release the reference driver's MD2PP isolation bit after settling.
        let regulator = self.read8(power::DIGITAL_REGULATOR)?;
        if regulator & power::DIGITAL_REGULATOR_ENABLED == 0 {
            self.write8(
                power::DIGITAL_REGULATOR,
                regulator | power::DIGITAL_REGULATOR_ENABLED,
            )?;
            delay_us(power::REGULATOR_SETTLE_US);
            self.clear_bits16(power::ISOLATION, power::MD2PP_ISOLATION)?;
        }

        // 4. Request power-on of the MAC (the dongle's Wi-Fi frame-processing
        // hardware). The dongle clears the request bit when this completes.
        self.set_bits16(power::STATE, power::REQUEST_MAC_POWER_ON)?;
        self.wait_for_bits16(
            power::STATE,
            power::REQUEST_MAC_POWER_ON,
            BitState::Clear,
            power::POLL_INTERVAL_US,
        )?;

        // 5. Apply the reference running configuration and release radio
        // digital isolation so the powered blocks can communicate.
        self.write16(power::STATE, power::RUNNING_CONFIGURATION)?;
        self.clear_bits16(power::ISOLATION, power::RADIO_DIGITAL_ISOLATION)?;

        // 6. Clear the MAC-off request, then wait for its off-status to clear.
        let sleep_control = self.read8(power::MAC_SLEEP_CONTROL)?;
        self.write8(
            power::MAC_SLEEP_CONTROL,
            sleep_control & !power::MAC_OFF_REQUEST,
        )?;
        self.wait_for_bits16(
            power::MAC_SLEEP_CONTROL,
            power::MAC_OFF_STATUS,
            BitState::Clear,
            power::MAC_POLL_INTERVAL_US,
        )?;

        // 7. Enable USB/data movement, packet scheduling, send/receive, and
        // encryption hardware. DMA means direct memory access inside the dongle.
        self.set_bits16(power::BLOCK_ENABLE, power::REQUIRED_BLOCKS)?;

        // 8. Retain the reference driver's final, undocumented register write.
        self.write8(
            power::UNDOCUMENTED_FINAL_REGISTER,
            power::UNDOCUMENTED_FINAL_VALUE,
        )
    }

    pub fn verify_identity_reads(&mut self, expected: u32) -> Result<(), Error> {
        for iteration in 0..4096 {
            let actual = self.read32(0x00f0)?;
            if actual != expected {
                return Err(Error::UnstableRegister {
                    iteration,
                    expected,
                    actual,
                });
            }
        }
        Ok(())
    }

    /// Partition the adapter's packet RAM and connect it to the USB queues.
    pub fn initialize_packet_memory(&mut self) -> Result<(), Error> {
        use packet_memory::*;

        self.phase = "packet memory";
        if self.endpoints.output_count != REQUIRED_OUT_PIPES {
            return Err(Error::Configuration);
        }
        // Pages 0..248 end with an end-of-list marker; 249..255 form a ring.
        // The marker value 255 and page index 255 have distinct roles.
        for page in 0u32..PAGE_COUNT {
            let next = match page {
                LAST_TRANSMIT_PAGE => END_OF_LIST,
                LAST_RING_PAGE => FIRST_RING_PAGE,
                _ => page + 1,
            };
            self.write32(LINK_COMMAND, LINK_WRITE | (page << LINK_PAGE_SHIFT) | next)?;
            let mut done = false;
            for _ in 0..WRITE_ATTEMPTS {
                if self.read32(LINK_COMMAND)? & LINK_OPERATION_MASK == 0 {
                    done = true;
                    break;
                }
                delay_us(WRITE_POLL_US);
            }
            if !done {
                return Err(Error::Configuration);
            }
        }
        // This dongle has high/normal priority bulk-OUT pipes. Reserve 12/2
        // pages for them and share the remaining 234 transmit pages.
        self.write8(NORMAL_QUEUE_PAGES_REGISTER, NORMAL_QUEUE_PAGES)?;
        self.write32(
            QUEUE_PAGES_REGISTER,
            LOAD_QUEUE_PAGES | (SHARED_QUEUE_PAGES << SHARED_PAGES_SHIFT) | HIGH_QUEUE_PAGES,
        )?;
        for register in BOUNDARY_REGISTERS {
            self.write8(register, FIRST_RING_PAGE as u8)?;
        }
        self.update16(DMA_QUEUE_MAP, QUEUE_MAP_MASK, HIGH_NORMAL_QUEUE_MAP)?;
        self.write16(RECEIVE_FIFO_BOUNDARY, RECEIVE_LAST_BYTE)?;
        self.write8(
            PAGE_SIZE_REGISTER,
            PAGE_SIZE_128_BYTES | (PAGE_SIZE_128_BYTES << TRANSMIT_PAGE_SIZE_SHIFT),
        )
    }

    /// Initialize one-chain baseband/RF for passive reception on 2.4 GHz.
    /// Transmit queues remain paused until association code configures them.
    pub fn initialize_receiver(&mut self, identity: &Identity) -> Result<(), Error> {
        self.phase = "receiver";
        // The tables below describe the standard USB dongle board.
        if identity.rom[0x79] >> 5 != 0 {
            return Err(Error::UnsupportedChip(identity.system_config));
        }
        for &(register, value) in tables::MAC {
            self.write8(register, value)?;
        }
        self.update16(0x0002, 0, 0x2003)?;
        self.write16(0x0028, 0xdb83)?;
        self.write8(0x001f, 7)?;
        self.write8(0x0002, 0x17)?;
        self.write8(0x0022, 0x0f)?;
        self.write8(0x0015, 0xe9)?;
        self.write8(0x0025, 0x80)?;
        for &(register, value) in tables::BASEBAND {
            self.write32(register, value)?;
            delay_us(1);
        }
        for &value in tables::GAIN {
            self.write32(0x0c78, value)?;
            delay_us(1);
        }
        let environment = self.read32(0x0870)? & 0x10;
        self.update32(0x0860, 0, 0x00100000)?;
        delay_us(50);
        self.update32(0x0860, 0, 0x10)?;
        delay_us(50);
        self.update32(0x0824, 0x400, 0)?;
        delay_us(50);
        self.update32(0x0824, 0x800, 0)?;
        delay_us(50);
        for &(register, value) in tables::RADIO {
            match register {
                0xfe | 0xffe => delay_us(50_000),
                0xfd => delay_us(5_000),
                0xfc => delay_us(1_000),
                0xfb => delay_us(50),
                0xfa => delay_us(5),
                0xf9 => delay_us(1),
                _ => {
                    self.radio_write(register as u8, value)?;
                    delay_us(5);
                }
            }
        }
        self.update32(0x0870, 0x10, environment)?;
        self.channel_register = self.radio_read(0x18)?;
        self.update32(0x0800, 0, 0x03000000)?; // Enable CCK and OFDM receivers.
        self.write8(0x0522, 0xff)?; // Pause transmit queues during passive scan.
        self.calibrate_iq()?;
        self.radio_write(0x18, self.channel_register | 0x8000)?; // LC calibration.
        delay_us(100_000);
        self.write8(0xfe40, 0xe0)?;
        self.write8(0xfe41, 0x8d)?;
        self.write8(0xfe42, 0x80)?;
        self.write(0x0610, &identity.mac)?;
        self.write8(0x060f, 4)?; // Four 8-byte PHY status words before each frame.
        self.write8(0x0102, 0)?; // Monitor network type.
        self.write32(0x0608, 0x7000600f)?; // Management frames + PHY status.
        self.write32(0x0620, u32::MAX)?;
        self.write32(0x0624, u32::MAX)?;
        self.write16(0x06a0, 0xffff)?;
        self.write16(0x06a2, 0)?;
        self.write16(0x06a4, 0)?;
        self.update16(0x010c, 4, 0)?; // Receive one USB frame without aggregation.
        self.set_channel(1)
    }

    fn update32(&mut self, register: u16, clear: u32, set: u32) -> Result<(), Error> {
        let value = self.read32(register)?;
        self.write32(register, (value & !clear) | set)
    }

    fn radio_write(&mut self, register: u8, value: u32) -> Result<(), Error> {
        self.write32(0x0840, ((register as u32) << 20) | (value & 0x000fffff))?;
        delay_us(1);
        Ok(())
    }

    fn radio_read(&mut self, register: u8) -> Result<u32, Error> {
        let saved = self.read32(0x0824)?;
        self.write32(0x0824, saved & !0x80000000)?;
        delay_us(1000);
        self.write32(
            0x0824,
            (saved & !0x7f800000) | ((register as u32) << 23) | 0x80000000,
        )?;
        delay_us(1000);
        self.write32(0x0824, saved | 0x80000000)?;
        delay_us(1000);
        let readback = if self.read32(0x0820)? & 0x100 != 0 {
            0x08b8
        } else {
            0x08a0
        };
        Ok(self.read32(readback)? & 0x000fffff)
    }

    pub fn set_channel(&mut self, channel: u8) -> Result<(), Error> {
        if !(1..=13).contains(&channel) {
            return Err(Error::Configuration);
        }
        let bandwidth = self.read8(0x0603)?;
        self.write8(0x0603, bandwidth | 4)?;
        self.update32(0x0800, 1, 0)?;
        self.update32(0x0900, 1, 0)?;
        self.update32(0x0884, 0, 0x400)?;
        self.radio_write(
            0x18,
            (self.channel_register & !0xfff) | channel as u32 | 0x400,
        )?;
        delay_us(10_000);
        Ok(())
    }

    /// Validate the adapter descriptor before exposing its 802.11 payload.
    pub fn receive_frame<'b>(&mut self, buffer: &'b mut [u8]) -> Option<ReceivedFrame<'b>> {
        let length = self.receive(buffer)?;
        if length < rx_descriptor::BYTES {
            return None;
        }
        let status = u32::from_le_bytes(buffer[..4].try_into().unwrap());
        if status & (rx_descriptor::CRC_ERROR | rx_descriptor::INTEGRITY_CHECK_ERROR) != 0 {
            return None;
        }
        let packet_length = (status & rx_descriptor::PACKET_LENGTH_MASK) as usize;
        // Skip the fixed descriptor, optional driver/PHY information and
        // alignment padding to reach the first byte of the 802.11 frame.
        let driver_info_bytes = ((status >> rx_descriptor::DRIVER_INFO_SHIFT)
            & rx_descriptor::DRIVER_INFO_MASK) as usize
            * rx_descriptor::DRIVER_INFO_UNIT_BYTES;
        let padding_bytes =
            ((status >> rx_descriptor::PADDING_SHIFT) & rx_descriptor::PADDING_MASK) as usize;
        let offset = rx_descriptor::BYTES + driver_info_bytes + padding_bytes;
        if offset + packet_length > length {
            return None;
        }
        Some(ReceivedFrame {
            bytes: &buffer[offset..offset + packet_length],
            hardware_decrypted: status & rx_descriptor::SOFTWARE_DECRYPTION_REQUIRED == 0,
        })
    }

    /// Receive a USB transfer containing the device descriptor and raw frame.
    pub fn receive(&mut self, buffer: &mut [u8]) -> Option<usize> {
        let length = unsafe {
            self.usb.bulk_in(
                self.device,
                self.endpoints.input,
                self.endpoints.max_packet_size,
                self.rx_pid,
                buffer,
            )
        }?;
        let packets = length
            .max(1)
            .div_ceil(self.endpoints.max_packet_size as usize);
        if packets & 1 != 0 {
            self.rx_pid ^= USB_DATA_PID_TOGGLE;
        }
        Some(length)
    }

    /// Configure a 20 MHz station using a fixed 1 Mbps transmit rate.
    /// The CCK power index comes from this adapter's factory eFuse calibration.
    pub fn prepare_station(
        &mut self,
        identity: &Identity,
        bssid: [u8; 6],
        channel: u8,
    ) -> Result<(), Error> {
        self.set_channel(channel)?;
        let group = if channel <= 3 {
            0
        } else if channel <= 9 {
            1
        } else {
            2
        };
        let power = identity.rom[0x5a + group];
        if power > 63 {
            return Err(Error::InvalidRom);
        }
        self.update32(0xe08, 0xff00, (power as u32) << 8)?;
        self.update32(0x86c, 0xffffff00, (power as u32) * 0x01010100)?;
        self.write(0x0618, &bssid)?;
        self.write8(0x0102, 2)?; // Infrastructure station network type.
        self.write16(0x06a4, 0xffff)?; // Admit data, including EAPOL.
        self.update32(0x0440, 0x000fffff, 0x000ffff1)?;
        self.write16(0x042a, 0x3030)?;
        self.write8(0x0640, 0x40)?;
        for register in [0x0428, 0x063a, 0x0514, 0x0516, 0x063c, 0x063e] {
            self.write16(register, 0x100a)?;
        }
        // Conservative contention settings for an initial non-QoS station.
        for register in [0x0500, 0x0504, 0x0508, 0x050c] {
            self.write32(register, 0x0000a42b)?;
        }
        self.write8(0x0522, 0)
    }

    /// Clear the hardware key table when starting a new association.
    pub fn clear_keys(&mut self) -> Result<(), Error> {
        self.phase = "clear keys";
        self.write32(key_table::COMMAND, key_table::BUSY | key_table::CLEAR_ALL)?;
        self.wait_key_table()?;
        self.write16(
            key_table::SECURITY_CONFIG,
            key_table::TX_DEFAULT_UNICAST_KEY
                | key_table::RX_DEFAULT_UNICAST_KEY
                | key_table::TX_ENCRYPTION
                | key_table::RX_DECRYPTION
                | key_table::TX_DEFAULT_BROADCAST_KEY
                | key_table::RX_DEFAULT_BROADCAST_KEY,
        )
    }

    /// Diagnostic receive filter for observing 802.11 acknowledgement frames.
    pub fn observe_acknowledgements(&mut self) -> Result<(), Error> {
        self.update32(0x0608, 0, 1 << 12)?; // RCR_ACF: admit control frames.
        self.write16(0x06a2, 1 << 13) // RXFLTMAP1: ACK subtype.
    }

    /// Register the station and broadcast rate tables with the adapter's MCU.
    /// Both start at CCK1; the descriptor also selects this fixed data rate.
    pub fn configure_station_rates(&mut self) -> Result<(), Error> {
        self.phase = "station rate registration";
        for station_id in [station_rate::BROADCAST_ID, station_rate::ACCESS_POINT_ID] {
            let mut payload = [0u8; 5];
            payload[..4].copy_from_slice(
                &(station_rate::TABLE_80211B | station_rate::CCK_RATE_MASK).to_le_bytes(),
            );
            payload[4] = station_rate::ID_VALID | station_id;
            self.firmware_command(station_rate::CONFIGURE_COMMAND, &payload)?;
            self.write8(
                station_rate::INITIAL_RATE_BASE + station_id as u16,
                station_rate::CCK1,
            )?;
        }
        Ok(())
    }

    /// Send one command to the dongle's firmware through a numbered mailbox.
    /// Main word: command byte + up to three payload bytes. Longer commands
    /// put their first two payload bytes in a separate extension register.
    fn firmware_command(&mut self, command: u8, payload: &[u8]) -> Result<(), Error> {
        if command & fw::COMMAND_HAS_EXTENSION != 0 || payload.len() > fw::MAX_COMMAND_PAYLOAD_BYTES
        {
            return Err(Error::Configuration);
        }
        let mailbox = self.firmware_mailbox as u16;
        // A clear status bit means firmware consumed this mailbox's previous
        // command. Wait before overwriting either half of that command.
        self.wait_for_bits16(
            fw::MAILBOX_STATUS,
            1 << mailbox,
            BitState::Clear,
            fw::MAILBOX_POLL_US,
        )?;
        let mut main = [0u8; fw::MAIN_WORD_BYTES];
        let mut extension = [0u8; fw::EXTENSION_BYTES];
        main[0] = command;
        if payload.len() > fw::INLINE_PAYLOAD_BYTES {
            // RTL8188C puts the first two payload bytes in the extension.
            main[0] |= fw::COMMAND_HAS_EXTENSION;
            extension.copy_from_slice(&payload[..fw::EXTENSION_BYTES]);
            main[1..payload.len() - 1].copy_from_slice(&payload[fw::EXTENSION_BYTES..]);
        } else {
            main[1..1 + payload.len()].copy_from_slice(payload);
        }
        self.write16(
            fw::MAILBOX_EXTENSION_BASE + mailbox * fw::EXTENSION_BYTES as u16,
            u16::from_le_bytes(extension),
        )?;
        // The main word alerts firmware, so write it after the extension.
        self.write32(
            fw::MAILBOX_BASE + mailbox * fw::MAIN_WORD_BYTES as u16,
            u32::from_le_bytes(main),
        )?;
        self.firmware_mailbox = (self.firmware_mailbox + 1) % fw::MAILBOX_COUNT;
        Ok(())
    }

    pub fn install_ccmp_key(
        &mut self,
        entry: u8,
        key_id: u8,
        mac: [u8; 6],
        key: &[u8; 16],
    ) -> Result<(), Error> {
        if entry > key_table::LAST_STATION_ENTRY || key_id > key_table::MAX_KEY_ID {
            return Err(Error::Configuration);
        }
        let base = entry as u16 * key_table::WORDS_PER_ENTRY;
        // Keep the entry invalid until all key and address words are present.
        self.write_key_word(base, 0)?;
        for (i, word) in key.chunks_exact(key_table::WORD_BYTES).enumerate() {
            self.write_key_word(
                base + key_table::KEY_FIRST_WORD + i as u16,
                u32::from_le_bytes(word.try_into().unwrap()),
            )?;
        }
        // Control word 1 holds the final four MAC bytes. Control word 0
        // combines the first two MAC bytes, algorithm, key ID and valid bit.
        self.write_key_word(base + 1, u32::from_le_bytes(mac[2..6].try_into().unwrap()))?;
        self.write_key_word(
            base,
            key_table::VALID
                | key_table::ALGORITHM_AES
                | key_id as u32
                | ((u16::from_le_bytes([mac[0], mac[1]]) as u32) << key_table::MAC_LOW_SHIFT),
        )
    }

    fn write_key_word(&mut self, address: u16, value: u32) -> Result<(), Error> {
        self.write32(key_table::WRITE_DATA, value)?;
        self.write32(
            key_table::COMMAND,
            key_table::BUSY | key_table::WRITE | address as u32,
        )?;
        self.wait_key_table()
    }
    // The dongle clears BUSY after finishing the requested table operation.
    fn wait_key_table(&mut self) -> Result<(), Error> {
        for _ in 0..key_table::POLL_ATTEMPTS {
            if self.read32(key_table::COMMAND)? & key_table::BUSY == 0 {
                return Ok(());
            }
            delay_us(key_table::POLL_DELAY_US);
        }
        Err(Error::Configuration)
    }

    /// Submit a raw frame through its management or data USB pipe.
    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), Error> {
        self.phase = "transmit";
        #[repr(align(64))]
        struct Packet([u8; tx_descriptor::USB_BUFFER_BYTES]);
        if frame.len() < WIFI_HEADER_BYTES
            || frame.len() > tx_descriptor::USB_BUFFER_BYTES - tx_descriptor::BYTES
        {
            return Err(Error::Configuration);
        }
        let data_frame = frame[0] & WIFI_FRAME_TYPE_MASK == WIFI_DATA_FRAME;
        // Management uses the high queue/first pipe; data uses normal/second.
        let pipe = usize::from(data_frame);
        let queue_flags = if data_frame {
            tx_descriptor::BEST_EFFORT_QUEUE | tx_descriptor::AGGREGATION_BREAK
        } else {
            tx_descriptor::MANAGEMENT_QUEUE
        };
        let mut packet = Packet([0; tx_descriptor::USB_BUFFER_BYTES]);
        let descriptor = &mut packet.0[..tx_descriptor::BYTES];
        // Word 0 gives the payload length and offset, and hands the complete
        // single-segment frame to the adapter. Address 1 begins at frame[4].
        let flags = tx_descriptor::OWNED_BY_ADAPTER
            | tx_descriptor::FIRST_SEGMENT
            | tx_descriptor::LAST_SEGMENT
            | (tx_descriptor::BYTES as u32) << tx_descriptor::FRAME_OFFSET_SHIFT
            | frame.len() as u32
            | if frame[4] & 1 != 0 {
                tx_descriptor::MULTICAST
            } else {
                0
            };
        descriptor[..4].copy_from_slice(&flags.to_le_bytes());
        let cipher = if frame[1] & WIFI_PROTECTED_FRAME != 0 {
            tx_descriptor::CIPHER_AES
        } else {
            0
        };
        descriptor[4..8].copy_from_slice(
            &(tx_descriptor::RATE_TABLE_80211B | queue_flags | cipher).to_le_bytes(),
        );
        let sequence = u16::from_le_bytes([frame[22], frame[23]]) >> 4;
        // The adapter assigns non-QoS sequence numbers across both queues.
        // HWSEQEN is in the sequence field; HWSEQ selects it in control word 4.
        descriptor[14..16]
            .copy_from_slice(&(sequence | tx_descriptor::SEQUENCE_ENABLE).to_le_bytes());
        descriptor[16..20].copy_from_slice(
            &(tx_descriptor::HARDWARE_SEQUENCE | tx_descriptor::USE_DRIVER_RATE).to_le_bytes(),
        );
        // The zero-initialized rate field in word 5 selects CCK1 (1 Mbps).
        if data_frame {
            // Match the reference driver's data/RTS rate fallback limits.
            descriptor[20..24].copy_from_slice(
                &(tx_descriptor::DATA_FALLBACK_LIMIT | tx_descriptor::RTS_FALLBACK_LIMIT)
                    .to_le_bytes(),
            );
        }
        // XOR all sixteen little-endian words while the checksum field is
        // zero, then fill that field so XOR over the complete header is zero.
        let checksum = descriptor.chunks_exact(2).fold(0u16, |sum, word| {
            sum ^ u16::from_le_bytes([word[0], word[1]])
        });
        descriptor[tx_descriptor::CHECKSUM_BYTES].copy_from_slice(&checksum.to_le_bytes());
        packet.0[tx_descriptor::BYTES..tx_descriptor::BYTES + frame.len()].copy_from_slice(frame);
        let length = frame.len() + tx_descriptor::BYTES;
        let endpoint = self.endpoints.outputs[pipe];
        let sent = unsafe {
            self.usb.bulk_out(
                self.device,
                endpoint,
                self.endpoints.max_packet_size,
                self.tx_pid[pipe],
                &packet.0[..length],
            )
        };
        if sent != Some(length) {
            return Err(Error::UsbTransmit(self.usb.channel_status()));
        }
        if length.div_ceil(self.endpoints.max_packet_size as usize) & 1 != 0 {
            self.tx_pid[pipe] ^= USB_DATA_PID_TOGGLE;
        }
        // A short USB packet terminates each adapter transmission. An exact
        // multiple of the endpoint size therefore needs a zero-length packet.
        if length % self.endpoints.max_packet_size as usize == 0 {
            if unsafe {
                self.usb.bulk_out(
                    self.device,
                    endpoint,
                    self.endpoints.max_packet_size,
                    self.tx_pid[pipe],
                    &packet.0[..0],
                )
            } != Some(0)
            {
                return Err(Error::Configuration);
            }
            self.tx_pid[pipe] ^= USB_DATA_PID_TOGGLE;
        }
        Ok(())
    }

    /// Copy the adapter's firmware into its own MCU RAM, then await its ready bit.
    pub fn load_firmware(&mut self, firmware: &[u8]) -> Result<(), Error> {
        if firmware.len() < fw::HEADER_BYTES
            || firmware.len() > fw::HEADER_BYTES + fw::PAGE_COUNT * fw::PAGE_BYTES
        {
            return Err(Error::InvalidFirmware);
        }
        // The file starts with a vendor header. Only the bytes after it are
        // executable firmware. The reference family check ignores the low four
        // signature bits.
        let signature =
            u16::from_le_bytes([firmware[0], firmware[1]]) >> fw::SIGNATURE_FAMILY_SHIFT;
        if signature != fw::SIGNATURE_8188C && signature != fw::SIGNATURE_8192C {
            return Err(Error::InvalidFirmware);
        }
        let payload = &firmware[fw::HEADER_BYTES..];
        self.phase = "firmware reset";
        if self.read8(fw::CONTROL)? & fw::RUNNING_FROM_RAM != 0 {
            // A USB reset can leave the radio MCU running. Hold it in reset
            // before replacing its program, then release it after verification.
            self.write8(fw::CONTROL, 0)?;
            self.write8(fw::SELF_RESET_REGISTER, fw::SELF_RESET_REQUEST)?; // Ask the running 8051 to reset itself.
            let mut reset = false;
            for _ in 0..fw::RESET_ATTEMPTS {
                if self.read16(fw::SYSTEM_FUNCTIONS)? & fw::PROCESSOR_ENABLED == 0 {
                    reset = true;
                    break;
                }
                delay_us(fw::RESET_POLL_US);
            }
            if !reset {
                self.clear_bits16(fw::SYSTEM_FUNCTIONS, fw::PROCESSOR_ENABLED)?;
            }
        }
        self.phase = "firmware download enable";
        let value = self.read8(fw::CONTROL)?;
        self.write8(fw::CONTROL, value | fw::DOWNLOAD_ENABLED)?;
        let value = self.read32(fw::CONTROL)?;
        self.write32(fw::CONTROL, value & !fw::ROM_DOWNLOAD_ENABLED)?;
        let value = self.read8(fw::CONTROL)?;
        self.write8(fw::CONTROL, value | fw::CHECKSUM_REPORTED)?; // Reset the firmware checksum report.
        delay_us(fw::DOWNLOAD_SETTLE_US);
        self.phase = "firmware pages";
        // Select one 4 KiB page at a time. The same USB-visible address window
        // accesses whichever page CONTROL currently selects. Read back each
        // write before advancing so a corrupt upload cannot be started.
        for (page, bytes) in payload.chunks(fw::PAGE_BYTES).enumerate() {
            let value = self.read32(fw::CONTROL)?;
            self.write32(
                fw::CONTROL,
                (value & !fw::PAGE_MASK) | ((page as u32) << fw::PAGE_SHIFT),
            )?;
            let mut offset = 0;
            while offset < bytes.len() {
                // Match the USB firmware loader's supported write sizes.
                let remaining = bytes.len() - offset;
                let length = if remaining > fw::USB_BLOCK_BYTES {
                    fw::USB_BLOCK_BYTES
                } else if remaining > fw::USB_WORD_BYTES {
                    fw::USB_WORD_BYTES
                } else {
                    1
                };
                self.write(
                    fw::WRITE_WINDOW + offset as u16,
                    &bytes[offset..offset + length],
                )?;
                let mut readback = [0u8; fw::USB_BLOCK_BYTES];
                let address = fw::WRITE_WINDOW + offset as u16;
                let received = unsafe {
                    self.usb.control_read(
                        self.device,
                        USB_VENDOR_DEVICE_READ,
                        REALTEK_REGISTER_REQUEST,
                        address,
                        UNUSED_REQUEST_INDEX,
                        &mut readback[..length],
                    )
                };
                if received != Some(length) {
                    return Err(Error::UsbRead(
                        address,
                        received,
                        self.usb.channel_status(),
                        self.phase,
                    ));
                }
                for index in 0..length {
                    if readback[index] != bytes[offset + index] {
                        delay_us(fw::READBACK_RETRY_US);
                        let mut repeated = [0u8; fw::USB_BLOCK_BYTES];
                        let _ = unsafe {
                            self.usb.control_read(
                                self.device,
                                USB_VENDOR_DEVICE_READ,
                                REALTEK_REGISTER_REQUEST,
                                address,
                                UNUSED_REQUEST_INDEX,
                                &mut repeated[..length],
                            )
                        };
                        return Err(Error::FirmwareReadback {
                            page: page as u8,
                            status: self.read32(fw::CONTROL)?,
                            address: address + index as u16,
                            expected: bytes[offset + index],
                            actual: readback[index],
                            repeated: repeated[index],
                        });
                    }
                }
                offset += length;
            }
        }
        // The dongle must report the download checksum before we close the
        // write window and restart its processor. INITIALIZATION_READY belongs
        // to the newly started firmware, so clear its previous value first.
        self.phase = "firmware checksum";
        self.wait_for_bits16(
            fw::CONTROL,
            fw::CHECKSUM_REPORTED as u16,
            BitState::Set,
            fw::CHECKSUM_POLL_US,
        )?;
        let value = self.read8(fw::CONTROL)?;
        self.write8(fw::CONTROL, value & !fw::DOWNLOAD_ENABLED)?;
        self.write8(fw::CONTROL + 1, 0)?;
        let value = self.read32(fw::CONTROL)?;
        self.write32(
            fw::CONTROL,
            (value & !(fw::INITIALIZATION_READY as u32)) | fw::DOWNLOAD_READY,
        )?;
        self.clear_bits16(fw::SYSTEM_FUNCTIONS, fw::PROCESSOR_ENABLED)?;
        self.set_bits16(fw::SYSTEM_FUNCTIONS, fw::PROCESSOR_ENABLED)?;
        delay_us(fw::PROCESSOR_START_US);
        self.phase = "firmware start";
        self.wait_for_bits16(
            fw::CONTROL,
            fw::INITIALIZATION_READY,
            BitState::Set,
            fw::READY_POLL_US,
        )
    }

    /// Poll a two-byte dongle register until all selected bits are set or clear.
    /// Every read is a USB control transfer. The interval is an extra delay
    /// between attempts; total elapsed time also includes those USB transfers.
    fn wait_for_bits16(
        &mut self,
        register: u16,
        mask: u16,
        state: BitState,
        interval_us: u32,
    ) -> Result<(), Error> {
        let expected = match state {
            BitState::Set => mask,
            BitState::Clear => 0,
        };
        for _ in 0..REGISTER_POLL_ATTEMPTS {
            if self.read16(register)? & mask == expected {
                return Ok(());
            }
            delay_us(interval_us);
        }
        Err(Error::Timeout {
            register,
            mask,
            expected,
            observed: self.read16(register)?,
        })
    }
}

fn delay_us(microseconds: u32) {
    let timer = 0x3f003004 as *const u32;
    let start = unsafe { core::ptr::read_volatile(timer) };
    while unsafe { core::ptr::read_volatile(timer) }.wrapping_sub(start) < microseconds {
        core::hint::spin_loop();
    }
}
