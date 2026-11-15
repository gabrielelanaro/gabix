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
    pub rom: [u8; 128],
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

    fn read<const N: usize>(&mut self, register: u16) -> Result<[u8; N], Error> {
        let mut bytes = [0; N];
        let received = unsafe {
            self.usb
                .control_read(self.device, 0xc0, 5, register, 0, &mut bytes)
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

    fn write(&mut self, register: u16, bytes: &[u8]) -> Result<(), Error> {
        unsafe {
            self.usb
                .control_write(self.device, 0x40, 5, register, 0, bytes)
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

    /// Read the factory-programmed identity and calibration bytes.
    /// These operations enable the eFuse reader; they never program a fuse.
    pub fn identify(&mut self) -> Result<Identity, Error> {
        let system_config = self.read32(0x00f0)?;
        // This implementation supports the production 8188C, one radio chain.
        if system_config & ((1 << 23) | (1 << 27)) != 0 {
            return Err(Error::UnsupportedChip(system_config));
        }
        self.update16(0x0000, 0, 0x8000)?; // Enable eFuse read supply.
        self.update16(0x0002, 0, 0x1000)?; // Release EEPROM loader reset.
        self.update16(0x0008, 0, 0x0022)?; // Enable loader and 8 MHz clocks.
        let mut rom = [0xff; 128];
        let mut address = 0u16;
        while address < 512 {
            let header = self.efuse_byte(address)?;
            address += 1;
            if header == 0xff {
                break;
            }
            let offset = (header >> 4) as usize * 8;
            for word in 0..4 {
                if header & (1 << word) != 0 {
                    continue;
                }
                if address + 1 >= 512 {
                    return Err(Error::InvalidRom);
                }
                rom[offset + word * 2] = self.efuse_byte(address)?;
                rom[offset + word * 2 + 1] = self.efuse_byte(address + 1)?;
                address += 2;
            }
        }
        if rom[0..2] != [0x29, 0x81] {
            return Err(Error::InvalidRom);
        }
        let mut mac = [0; 6];
        mac.copy_from_slice(&rom[22..28]);
        if mac[0] & 1 != 0 || mac == [0; 6] {
            return Err(Error::InvalidRom);
        }
        Ok(Identity {
            system_config,
            mac,
            rom,
            pa_setting: self.efuse_byte(0x1fa)?,
        })
    }

    fn efuse_byte(&mut self, address: u16) -> Result<u8, Error> {
        const EFUSE_CTRL: u16 = 0x0030;
        let value = self.read32(EFUSE_CTRL)?;
        self.write32(EFUSE_CTRL, (value & !0x8003ff00) | ((address as u32) << 8))?;
        for _ in 0..100 {
            let value = self.read32(EFUSE_CTRL)?;
            if value & (1 << 31) != 0 {
                return Ok(value as u8);
            }
            delay_us(5);
        }
        Err(Error::Timeout {
            register: EFUSE_CTRL,
            mask: 0x8000,
            expected: 0x8000,
            observed: (self.read32(EFUSE_CTRL)? >> 16) as u16,
        })
    }

    /// Return a warm adapter to the documented power-off state before init.
    /// A Pi reboot resets USB but can leave the dongle's 8051 and RF running.
    pub fn power_off(&mut self) -> Result<(), Error> {
        self.phase = "power off";
        self.write8(0x0522, 0xff)?;
        self.radio_write(0, 0)?;
        self.write8(0x0600, 0x40)?;
        self.write8(0x0002, 0x16)?;
        self.write8(0x0002, 0x14)?;
        if self.read8(0x0080)? & 2 != 0 {
            self.write8(0x0080, 0)?;
            self.write8(0x01cf, 0x20)?;
            for _ in 0..100 {
                if self.read16(0x0002)? & 0x0400 == 0 {
                    break;
                }
                delay_us(50);
            }
            self.update16(0x0002, 0x0400, 0)?;
        }
        self.write8(0x0003, 0x54)?;
        self.write8(0x0080, 0)?;
        self.write16(0x0008, 0x70a3)?;
        self.write8(0x0028, 0x80)?;
        self.write16(0x0024, 0x880f)?;
        self.write8(0x0000, 0xf9)?;
        self.write16(0x0046, 0)?;
        let gpio = self.read32(0x0044)? & !0xff00;
        self.write32(0x0044, gpio | ((gpio << 8) & 0xff00) | 0x00ff0000)?;
        self.write8(0x0043, 0)?;
        let gpio = self.read16(0x0042)? & !0x00f0;
        self.write16(0x0042, gpio | ((gpio & 15) << 4) | 0x0780)?;
        self.write16(0x004c, 0x8080)?;
        self.write16(0x0008, 0x70a3)?;
        self.write8(0x0001, 0x82)?;
        self.write8(0x0020, 4)?;
        let regulator = self.read8(0x0021)?;
        self.write8(0x0021, regulator & !1)?;
        self.write8(0x0011, 0x23)?;
        self.write16(0x0004, 0x4802)?;
        self.write8(0x001c, 0x0e)
    }

    /// Power the MAC and radio control blocks. Transmission stays unconfigured.
    pub fn power_on(&mut self) -> Result<(), Error> {
        self.phase = "power on";
        const APS_FSMCO: u16 = 0x0004;
        const SYS_ISO_CTRL: u16 = 0x0000;
        const LDOV12D_CTRL: u16 = 0x0021;
        const APSD_CTRL: u16 = 0x0600;
        self.wait16(APS_FSMCO, 2, 2, 5)?;
        self.write8(0x001c, 0)?; // Unlock clock and power control.
        self.write8(0x0011, 0x2b)?; // Switching regulator PWM mode.
        delay_us(100);
        let regulator = self.read8(LDOV12D_CTRL)?;
        if regulator & 1 == 0 {
            self.write8(LDOV12D_CTRL, regulator | 1)?;
            delay_us(100);
            self.update16(SYS_ISO_CTRL, 1, 0)?;
        }
        self.update16(APS_FSMCO, 0, 0x0100)?;
        self.wait16(APS_FSMCO, 0x0100, 0, 5)?;
        self.write16(APS_FSMCO, 0x0812)?;
        self.update16(SYS_ISO_CTRL, 0x0200, 0)?;
        let sleep_control = self.read8(APSD_CTRL)?;
        self.write8(APSD_CTRL, sleep_control & !0x40)?;
        self.wait16(APSD_CTRL, 0x80, 0, 50)?;
        self.update16(0x0100, 0, 0x02ff)?; // DMA, scheduler, MAC and security blocks.
        self.write8(0xfe10, 0x19)
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
        self.phase = "packet memory";
        if self.endpoints.output_count != 2 {
            return Err(Error::Configuration);
        }
        // Pages 0..248 form the transmit list; 249..255 form the receive ring.
        for page in 0u32..256 {
            let next = match page {
                248 => 255,
                255 => 249,
                _ => page + 1,
            };
            self.write32(0x01e0, (1 << 30) | (page << 8) | next)?;
            let mut done = false;
            for _ in 0..20 {
                if self.read32(0x01e0)? & 0xc0000000 == 0 {
                    done = true;
                    break;
                }
                delay_us(5);
            }
            if !done {
                return Err(Error::Configuration);
            }
        }
        // This dongle has high/normal priority bulk-OUT pipes. Reserve 12/2
        // pages for them and share the remaining 234 transmit pages.
        self.write8(0x0214, 2)?;
        self.write32(0x0200, 0x80000000 | (234 << 16) | 12)?;
        for register in [0x0424, 0x0425, 0x045d, 0x0114, 0x0209] {
            self.write8(register, 249)?;
        }
        self.update16(0x010c, 0xfff0, 0xfaf0)?;
        self.write16(0x0116, 0x27ff)?;
        self.write8(0x0104, 0x11)
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
        if length < 24 {
            return None;
        }
        let status = u32::from_le_bytes(buffer[..4].try_into().unwrap());
        if status & 0xc000 != 0 {
            return None;
        } // CRC or integrity-check failure.
        let packet_length = (status & 0x3fff) as usize;
        let offset = 24 + (((status >> 16) & 15) as usize) * 8 + ((status >> 24) & 3) as usize;
        if offset + packet_length > length {
            return None;
        }
        Some(ReceivedFrame {
            bytes: &buffer[offset..offset + packet_length],
            hardware_decrypted: status & 0x08000000 == 0,
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
            self.rx_pid ^= 2;
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
        self.write32(0x0670, 0xc0000000)?;
        self.wait_key_table()?;
        self.write16(0x0680, 0x00cf)
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
        for station_id in [4u8, 0] {
            let mut payload = [0u8; 5];
            payload[..4].copy_from_slice(&0x6000000fu32.to_le_bytes()); // 11b, 1/2/5.5/11 Mbps.
            payload[4] = 0x80 | station_id; // MACID_VALID.
            self.firmware_command(6, &payload)?; // MACID_CONFIG.
            self.write8(0x0484 + station_id as u16, 0)?;
        }
        Ok(())
    }

    fn firmware_command(&mut self, command: u8, payload: &[u8]) -> Result<(), Error> {
        if command & 0x80 != 0 || payload.len() > 5 {
            return Err(Error::Configuration);
        }
        let mailbox = self.firmware_mailbox as u16;
        self.wait16(0x01cc, 1 << mailbox, 0, 10)?;
        let mut main = [0u8; 4];
        let mut extension = [0u8; 2];
        main[0] = command;
        if payload.len() > 3 {
            // RTL8188C puts the first two payload bytes in the extension.
            main[0] |= 0x80;
            extension.copy_from_slice(&payload[..2]);
            main[1..payload.len() - 1].copy_from_slice(&payload[2..]);
        } else {
            main[1..1 + payload.len()].copy_from_slice(payload);
        }
        self.write16(0x0088 + mailbox * 2, u16::from_le_bytes(extension))?;
        // The main word alerts firmware, so write it after the extension.
        self.write32(0x01d0 + mailbox * 4, u32::from_le_bytes(main))?;
        self.firmware_mailbox = (self.firmware_mailbox + 1) % 4;
        Ok(())
    }

    pub fn install_ccmp_key(
        &mut self,
        entry: u8,
        key_id: u8,
        mac: [u8; 6],
        key: &[u8; 16],
    ) -> Result<(), Error> {
        if entry > 4 || key_id > 3 {
            return Err(Error::Configuration);
        }
        let base = entry as u16 * 8;
        // Keep the entry invalid until all key and address words are present.
        self.write_key_word(base, 0)?;
        for (i, word) in key.chunks_exact(4).enumerate() {
            self.write_key_word(
                base + 2 + i as u16,
                u32::from_le_bytes(word.try_into().unwrap()),
            )?;
        }
        self.write_key_word(base + 1, u32::from_le_bytes(mac[2..6].try_into().unwrap()))?;
        self.write_key_word(
            base,
            0x8010 | key_id as u32 | ((u16::from_le_bytes([mac[0], mac[1]]) as u32) << 16),
        )
    }

    fn write_key_word(&mut self, address: u16, value: u32) -> Result<(), Error> {
        self.write32(0x0674, value)?;
        self.write32(0x0670, 0x80010000 | address as u32)?;
        self.wait_key_table()
    }
    fn wait_key_table(&mut self) -> Result<(), Error> {
        for _ in 0..1000 {
            if self.read32(0x0670)? & 0x80000000 == 0 {
                return Ok(());
            }
            delay_us(5);
        }
        Err(Error::Configuration)
    }

    /// Submit a raw frame through its management or data USB pipe.
    pub fn transmit(&mut self, frame: &[u8]) -> Result<(), Error> {
        self.phase = "transmit";
        #[repr(align(64))]
        struct Packet([u8; 2048]);
        if frame.len() < 24 || frame.len() > 2016 {
            return Err(Error::Configuration);
        }
        let data_frame = frame[0] & 0x0c == 0x08;
        // Management uses the high queue/first pipe; data uses normal/second.
        let pipe = usize::from(data_frame);
        let queue = if data_frame { 0x40 } else { 0x1200 };
        let mut packet = Packet([0; 2048]);
        let descriptor = &mut packet.0[..32];
        let flags =
            0x8c200000 | frame.len() as u32 | if frame[4] & 1 != 0 { 0x01000000 } else { 0 };
        descriptor[..4].copy_from_slice(&flags.to_le_bytes());
        let cipher = if frame[1] & 0x40 != 0 { 3 << 22 } else { 0 };
        descriptor[4..8].copy_from_slice(&(0x00060000u32 | queue | cipher).to_le_bytes());
        let sequence = u16::from_le_bytes([frame[22], frame[23]]) >> 4;
        // The adapter assigns non-QoS sequence numbers across both queues.
        // HWSEQEN is in the sequence field; HWSEQ selects it in control word 4.
        descriptor[14..16].copy_from_slice(&(sequence | 0x8000).to_le_bytes());
        descriptor[16..20].copy_from_slice(&0x180u32.to_le_bytes()); // HW sequence, CCK1.
        if data_frame {
            // Match the reference driver's data/RTS rate fallback limits.
            descriptor[20..24].copy_from_slice(&0x0001ff00u32.to_le_bytes());
        }
        let checksum = descriptor.chunks_exact(2).fold(0u16, |sum, word| {
            sum ^ u16::from_le_bytes([word[0], word[1]])
        });
        descriptor[28..30].copy_from_slice(&checksum.to_le_bytes());
        packet.0[32..32 + frame.len()].copy_from_slice(frame);
        let length = frame.len() + 32;
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
            self.tx_pid[pipe] ^= 2;
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
            self.tx_pid[pipe] ^= 2;
        }
        Ok(())
    }

    /// Copy the adapter's firmware into its own MCU RAM, then await its ready bit.
    pub fn load_firmware(&mut self, firmware: &[u8]) -> Result<(), Error> {
        const MCUFWDL: u16 = 0x0080;
        const SYS_FUNC_EN: u16 = 0x0002;
        if firmware.len() < 32 || firmware.len() > 32 + 8 * 4096 {
            return Err(Error::InvalidFirmware);
        }
        let signature = u16::from_le_bytes([firmware[0], firmware[1]]) >> 4;
        if signature != 0x88c && signature != 0x92c {
            return Err(Error::InvalidFirmware);
        }
        let payload = &firmware[32..];
        self.phase = "firmware reset";
        if self.read8(MCUFWDL)? & 0x80 != 0 {
            // A USB reset can leave the radio MCU running. Hold it in reset
            // before replacing its program, then release it after verification.
            self.write8(MCUFWDL, 0)?;
            self.write8(0x01cf, 0x20)?; // Ask the running 8051 to reset itself.
            let mut reset = false;
            for _ in 0..100 {
                if self.read16(SYS_FUNC_EN)? & 0x0400 == 0 {
                    reset = true;
                    break;
                }
                delay_us(50);
            }
            if !reset {
                self.update16(SYS_FUNC_EN, 0x0400, 0)?;
            }
        }
        self.phase = "firmware download enable";
        let value = self.read8(MCUFWDL)?;
        self.write8(MCUFWDL, value | 1)?;
        let value = self.read32(MCUFWDL)?;
        self.write32(MCUFWDL, value & !0x00080000)?;
        let value = self.read8(MCUFWDL)?;
        self.write8(MCUFWDL, value | 4)?; // Reset the firmware checksum report.
        delay_us(50);
        self.phase = "firmware pages";
        for (page, bytes) in payload.chunks(4096).enumerate() {
            let value = self.read32(MCUFWDL)?;
            self.write32(MCUFWDL, (value & !0x00070000) | ((page as u32) << 16))?;
            let mut offset = 0;
            while offset < bytes.len() {
                // Match the USB firmware loader's supported write sizes.
                let remaining = bytes.len() - offset;
                let length = if remaining > 196 {
                    196
                } else if remaining > 4 {
                    4
                } else {
                    1
                };
                self.write(0x1000 + offset as u16, &bytes[offset..offset + length])?;
                let mut readback = [0u8; 196];
                let address = 0x1000 + offset as u16;
                let received = unsafe {
                    self.usb
                        .control_read(self.device, 0xc0, 5, address, 0, &mut readback[..length])
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
                        delay_us(100);
                        let mut repeated = [0u8; 196];
                        let _ = unsafe {
                            self.usb.control_read(
                                self.device,
                                0xc0,
                                5,
                                address,
                                0,
                                &mut repeated[..length],
                            )
                        };
                        return Err(Error::FirmwareReadback {
                            page: page as u8,
                            status: self.read32(MCUFWDL)?,
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
        self.phase = "firmware checksum";
        self.wait16(MCUFWDL, 4, 4, 5)?;
        let value = self.read8(MCUFWDL)?;
        self.write8(MCUFWDL, value & !1)?;
        self.write8(MCUFWDL + 1, 0)?;
        let value = self.read32(MCUFWDL)?;
        self.write32(MCUFWDL, (value & !0x40) | 2)?;
        self.update16(SYS_FUNC_EN, 0x0400, 0)?;
        self.update16(SYS_FUNC_EN, 0, 0x0400)?;
        delay_us(5_000);
        self.phase = "firmware start";
        self.wait16(MCUFWDL, 0x40, 0x40, 10)
    }

    fn wait16(
        &mut self,
        register: u16,
        mask: u16,
        expected: u16,
        interval_us: u32,
    ) -> Result<(), Error> {
        for _ in 0..1000 {
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
