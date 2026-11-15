//! Single-chain RTL8188C I/Q calibration, adapted from OpenBSD rtwn.c.
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
use super::{delay_us, Error, Rtl8188cu};

const ANALOG_REGISTERS: [u16; 16] = [
    0x85c, 0xe6c, 0xe70, 0xe74, 0xe78, 0xe7c, 0xe80, 0xe84, 0xe88, 0xe8c, 0xed0, 0xed4, 0xed8,
    0xedc, 0xee0, 0xeec,
];
const BASEBAND_REGISTERS: [u16; 10] = [
    0xc04, 0xc08, 0x870, 0x874, 0x860, 0x864, 0xb68, 0xb6c, 0xa04, 0x820,
];

impl Rtl8188cu {
    /// Measure I/Q imbalance twice and apply only consistent results.
    /// Calibration temporarily routes an internal test signal through the RF
    /// chain. Save the normal receiver configuration before each measurement.
    pub fn calibrate_iq(&mut self) -> Result<(), Error> {
        self.phase = "I/Q calibration";
        let mut previous: Option<([u16; 4], u8)> = None;
        let mut transmit_only = None;
        let mut samples = [[0u16; 4]; 3];
        for sample in &mut samples {
            let current = self.measure_iq()?;
            *sample = current.0;
            if let Some((a, valid_a)) = previous {
                let (b, valid_b) = current;
                // Quadrature coefficients are signed 10-bit: 1022 is -2.
                let signed = |v: u16| ((v as i16) << 6) >> 6;
                let tx_stable = valid_a & valid_b & 1 != 0
                    && a[0].abs_diff(b[0]) <= 5
                    && signed(a[1]).abs_diff(signed(b[1])) <= 5;
                let rx_stable = valid_a & valid_b & 2 != 0
                    && a[2].abs_diff(b[2]) <= 5
                    && signed(a[3]).abs_diff(signed(b[3])) <= 5;
                if tx_stable {
                    if rx_stable {
                        self.rx_iq_calibrated = true;
                        return self.apply_iq(b, true);
                    }
                    transmit_only = Some(b);
                }
            }
            previous = Some(current);
        }
        // The reference drivers support TX-only correction. Preserve the
        // receiver's table defaults when its measured correction is unstable.
        if let Some(values) = transmit_only {
            self.rx_iq_calibrated = false;
            return self.apply_iq(values, false);
        }
        Err(Error::Calibration(samples))
    }

    fn measure_iq(&mut self) -> Result<([u16; 4], u8), Error> {
        let mut analog = [0; 16];
        let mut baseband = [0; 10];
        for (i, address) in ANALOG_REGISTERS.iter().enumerate() {
            analog[i] = self.read32(*address)?;
        }
        for (i, address) in BASEBAND_REGISTERS.iter().enumerate() {
            baseband[i] = self.read32(*address)?;
        }
        let hssi_b = self.read32(0x828)?;
        let pause = self.read8(0x522)?;
        let beacon = self.read8(0x550)?;
        let beacon_b = self.read8(0x551)?;
        let gpio = self.read32(0x040)?;
        let gain = self.read32(0xc50)?;
        let measured = (|| {
            for (i, address) in ANALOG_REGISTERS.iter().enumerate() {
                self.write32(*address, if i == 0 { 0x0b1b25a0 } else { 0x0bdb25a0 })?;
            }
            self.write32(0x820, baseband[9] | 0x100)?;
            self.write32(0x828, hssi_b | 0x100)?;
            self.write32(0xc04, 0x03a05600)?;
            self.write32(0xc08, 0x000800e4)?;
            self.write32(0x874, 0x22204000)?;
            self.write8(0x522, 0x3f)?;
            self.write8(0x550, beacon & !8)?;
            self.write8(0x551, beacon_b & !8)?;
            self.write8(0x040, gpio as u8 & !0x20)?;
            self.write32(0xb68, 0x00080000)?;
            self.write32(0xe28, 0x80800000)?;
            self.write32(0xe40, 0x01007c00)?;
            self.write32(0xe44, 0x01004800)?;
            let mut result = ([0; 4], 0);
            for _ in 0..2 {
                for (address, value) in [
                    (0xe30, 0x10008c1f),
                    (0xe54, 0x10008c1f),
                    (0xe38, 0x82140102),
                    (0xe3c, 0x28160502),
                    (0xe4c, 0x001028d1),
                    (0xe48, 0xf9000000),
                    (0xe48, 0xf8000000),
                ] {
                    self.write32(address, value)?;
                }
                delay_us(1000);
                let status = self.read32(0xeac)?;
                let mut values = [0u16; 4];
                for (i, address) in [0xe94, 0xe9c, 0xea4, 0xeac].iter().enumerate() {
                    values[i] = ((self.read32(*address)? >> 16) & 0x3ff) as u16;
                }
                let tx_valid = status & 0x10000000 == 0 && values[0] != 0x142 && values[1] != 0x42;
                let rx_valid = status & 0x08000000 == 0 && values[2] != 0x132 && values[3] != 0x36;
                result = (values, u8::from(tx_valid) | (u8::from(rx_valid) << 1));
            }
            Ok(result)
        })();
        // Restore receiver state even when the measurement reports failure.
        self.write32(0xe28, 0)?;
        self.write32(0x840, 0x00032ed3)?;
        for (address, value) in ANALOG_REGISTERS.iter().zip(analog) {
            self.write32(*address, value)?;
        }
        for (address, value) in BASEBAND_REGISTERS.iter().zip(baseband) {
            self.write32(*address, value)?;
        }
        self.write32(0x828, hssi_b)?;
        self.write8(0x522, pause)?;
        self.write8(0x550, beacon)?;
        self.write8(0x551, beacon_b)?;
        self.write32(0x040, gpio)?;
        self.write32(0xc50, (gain & !0xff) | 0x50)?;
        self.write32(0xc50, gain)?;
        self.write32(0xe30, 0x01008c00)?;
        self.write32(0xe34, 0x01008c00)?;
        measured
    }

    fn apply_iq(&mut self, values: [u16; 4], receive: bool) -> Result<(), Error> {
        let scale = ((self.read32(0xc80)? >> 22) & 0x3ff) as i32;
        let signed = |value: u16| ((value as i32) << 22) >> 22;
        let x = signed(values[0]) * scale;
        let y = signed(values[1]) * scale;
        self.update32(0xc80, 0x3ff, ((x >> 8) as u32) & 0x3ff)?;
        self.update32(0xc4c, 0x80000000, (((x >> 7) as u32) & 1) << 31)?;
        self.update32(0xc94, 0xf0000000, (((y >> 8) as u32) & 0x3c0) << 22)?;
        self.update32(0xc80, 0x003f0000, (((y >> 8) as u32) & 0x3f) << 16)?;
        self.update32(0xc4c, 0x20000000, (((y >> 7) as u32) & 1) << 29)?;
        if !receive {
            return Ok(());
        }
        self.update32(
            0xc14,
            0xffff,
            values[2] as u32 | ((values[3] as u32 & 0x3f) << 10),
        )?;
        self.update32(0xca0, 0xf0000000, (values[3] as u32 & 0x3c0) << 22)
    }
}
