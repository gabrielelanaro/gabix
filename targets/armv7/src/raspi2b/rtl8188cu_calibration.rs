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

// Register names follow OpenBSD r92creg.h; A/B identify radio paths.
mod reg {
    pub const IQ_CALIBRATION_MODE: u16 = 0xe28;
    pub const TX_TEST_TONE_A: u16 = 0xe30;
    pub const RX_TEST_TONE_A: u16 = 0xe34;
    pub const TX_IQK_PI_A: u16 = 0xe38;
    pub const RX_IQK_PI_A: u16 = 0xe3c;
    pub const TX_CALIBRATION: u16 = 0xe40;
    pub const RX_CALIBRATION: u16 = 0xe44;
    pub const IQK_AGC_POINTS: u16 = 0xe48;
    pub const IQK_AGC_RESPONSE: u16 = 0xe4c;
    pub const RX_TEST_TONE_B: u16 = 0xe54;
    pub const TX_RESULT_BEFORE: u16 = 0xe94;
    pub const TX_RESULT_AFTER: u16 = 0xe9c;
    pub const RX_RESULT_BEFORE: u16 = 0xea4;
    pub const RX_RESULT_AFTER_AND_STATUS: u16 = 0xeac;
    pub const TX_IQ_IMBALANCE: u16 = 0xc80;
    pub const ECCA_THRESHOLD: u16 = 0xc4c;
    pub const TX_ANALOG_FRONT_END: u16 = 0xc94;
    pub const RX_IQ_IMBALANCE: u16 = 0xc14;
    pub const RX_IQ_EXTENSION: u16 = 0xca0;
    pub const GAIN_CONTROL_A: u16 = 0xc50;
    pub const HSSI_PARAMETER_A: u16 = 0x820;
    pub const HSSI_PARAMETER_B: u16 = 0x828;
    pub const TX_PAUSE: u16 = 0x522;
    pub const BEACON_CONTROL_A: u16 = 0x550;
    pub const BEACON_CONTROL_B: u16 = 0x551;
    pub const ANTENNA_CONFIG_A: u16 = 0xb68;
    pub const LSSI_PARAMETER_A: u16 = 0x840;
}

// Result array order: transmit in-phase/quadrature, receive in-phase/quadrature.
const TX_I: usize = 0;
const TX_Q: usize = 1;
const RX_I: usize = 2;
const RX_Q: usize = 3;
const TX_VALID: u8 = 1;
const RX_VALID: u8 = 2;
const TX_FAILED: u32 = 1 << 28;
const RX_FAILED: u32 = 1 << 27;
const COEFFICIENT_MASK: u32 = 0x3ff;
const RESULT_SHIFT: u32 = 16;
const SAMPLE_TOLERANCE: u16 = 5;
const MEASUREMENT_ROUNDS: usize = 3;
const TRIGGERS_PER_ROUND: usize = 2;
const MEASUREMENT_SETTLE_US: u32 = 1000;
// The reference driver rejects these result values in addition to failure bits.
const TX_I_FAILURE_VALUE: u16 = 0x142;
const TX_Q_FAILURE_VALUE: u16 = 0x42;
const RX_I_FAILURE_VALUE: u16 = 0x132;
const RX_Q_FAILURE_VALUE: u16 = 0x36;

#[derive(Clone, Copy)]
enum ReceiveCorrection {
    Apply,
    KeepDefaults,
}

const ANALOG_REGISTERS: [u16; 16] = [
    0x85c, 0xe6c, 0xe70, 0xe74, 0xe78, 0xe7c, 0xe80, 0xe84, 0xe88, 0xe8c, 0xed0, 0xed4, 0xed8,
    0xedc, 0xee0, 0xeec,
];
const BASEBAND_REGISTERS: [u16; 10] = [
    0xc04,
    0xc08,
    0x870,
    0x874,
    0x860,
    0x864,
    reg::ANTENNA_CONFIG_A,
    0xb6c,
    0xa04,
    reg::HSSI_PARAMETER_A,
];

impl Rtl8188cu {
    /// Compare consecutive I/Q measurements, using at most three rounds.
    /// Calibration temporarily routes an internal test signal through the RF
    /// chain. Save the normal receiver configuration before each measurement.
    pub fn calibrate_iq(&mut self) -> Result<(), Error> {
        self.phase = "I/Q calibration";
        let mut previous: Option<([u16; 4], u8)> = None;
        let mut transmit_only = None;
        let mut samples = [[0u16; 4]; MEASUREMENT_ROUNDS];
        for sample in &mut samples {
            let current = self.measure_iq()?;
            *sample = current.0;
            if let Some((a, valid_a)) = previous {
                let (b, valid_b) = current;
                // Quadrature coefficients are signed 10-bit: 1022 is -2.
                let signed = |v: u16| ((v as i16) << 6) >> 6;
                let tx_stable = valid_a & valid_b & TX_VALID != 0
                    && a[TX_I].abs_diff(b[TX_I]) <= SAMPLE_TOLERANCE
                    && signed(a[TX_Q]).abs_diff(signed(b[TX_Q])) <= SAMPLE_TOLERANCE;
                let rx_stable = valid_a & valid_b & RX_VALID != 0
                    && a[RX_I].abs_diff(b[RX_I]) <= SAMPLE_TOLERANCE
                    && signed(a[RX_Q]).abs_diff(signed(b[RX_Q])) <= SAMPLE_TOLERANCE;
                if tx_stable {
                    if rx_stable {
                        self.rx_iq_calibrated = true;
                        return self.apply_iq(b, ReceiveCorrection::Apply);
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
            return self.apply_iq(values, ReceiveCorrection::KeepDefaults);
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
        let hssi_b = self.read32(reg::HSSI_PARAMETER_B)?;
        let pause = self.read8(reg::TX_PAUSE)?;
        let beacon = self.read8(reg::BEACON_CONTROL_A)?;
        let beacon_b = self.read8(reg::BEACON_CONTROL_B)?;
        let gpio = self.read32(0x040)?;
        let gain = self.read32(reg::GAIN_CONTROL_A)?;
        // Reference calibration recipe: preserve these tuning words and their
        // write order. Their individual analog settings are not decoded here.
        let measured = (|| {
            for (i, address) in ANALOG_REGISTERS.iter().enumerate() {
                self.write32(*address, if i == 0 { 0x0b1b25a0 } else { 0x0bdb25a0 })?;
            }
            self.write32(reg::HSSI_PARAMETER_A, baseband[9] | 0x100)?;
            self.write32(reg::HSSI_PARAMETER_B, hssi_b | 0x100)?;
            self.write32(0xc04, 0x03a05600)?;
            self.write32(0xc08, 0x000800e4)?;
            self.write32(0x874, 0x22204000)?;
            self.write8(reg::TX_PAUSE, 0x3f)?;
            self.write8(reg::BEACON_CONTROL_A, beacon & !8)?;
            self.write8(reg::BEACON_CONTROL_B, beacon_b & !8)?;
            self.write8(0x040, gpio as u8 & !0x20)?;
            self.write32(reg::ANTENNA_CONFIG_A, 0x00080000)?;
            self.write32(reg::IQ_CALIBRATION_MODE, 0x80800000)?;
            self.write32(reg::TX_CALIBRATION, 0x01007c00)?;
            self.write32(reg::RX_CALIBRATION, 0x01004800)?;
            let mut result = ([0; 4], 0);
            for _ in 0..TRIGGERS_PER_ROUND {
                for (address, value) in [
                    (reg::TX_TEST_TONE_A, 0x10008c1f),
                    (reg::RX_TEST_TONE_B, 0x10008c1f),
                    (reg::TX_IQK_PI_A, 0x82140102),
                    (reg::RX_IQK_PI_A, 0x28160502),
                    (reg::IQK_AGC_RESPONSE, 0x001028d1),
                    (reg::IQK_AGC_POINTS, 0xf9000000),
                    (reg::IQK_AGC_POINTS, 0xf8000000),
                ] {
                    self.write32(address, value)?;
                }
                delay_us(MEASUREMENT_SETTLE_US);
                let status = self.read32(reg::RX_RESULT_AFTER_AND_STATUS)?;
                let mut values = [0u16; 4];
                for (i, address) in [
                    reg::TX_RESULT_BEFORE,
                    reg::TX_RESULT_AFTER,
                    reg::RX_RESULT_BEFORE,
                    reg::RX_RESULT_AFTER_AND_STATUS,
                ]
                .iter()
                .enumerate()
                {
                    values[i] =
                        ((self.read32(*address)? >> RESULT_SHIFT) & COEFFICIENT_MASK) as u16;
                }
                let tx_valid = status & TX_FAILED == 0
                    && values[TX_I] != TX_I_FAILURE_VALUE
                    && values[TX_Q] != TX_Q_FAILURE_VALUE;
                let rx_valid = status & RX_FAILED == 0
                    && values[RX_I] != RX_I_FAILURE_VALUE
                    && values[RX_Q] != RX_Q_FAILURE_VALUE;
                result = (
                    values,
                    (if tx_valid { TX_VALID } else { 0 }) | (if rx_valid { RX_VALID } else { 0 }),
                );
            }
            // Preserve the existing policy: each round uses its second trigger.
            Ok(result)
        })();
        // Restore receiver state even when the measurement reports failure.
        self.write32(reg::IQ_CALIBRATION_MODE, 0)?;
        self.write32(reg::LSSI_PARAMETER_A, 0x00032ed3)?;
        for (address, value) in ANALOG_REGISTERS.iter().zip(analog) {
            self.write32(*address, value)?;
        }
        for (address, value) in BASEBAND_REGISTERS.iter().zip(baseband) {
            self.write32(*address, value)?;
        }
        self.write32(reg::HSSI_PARAMETER_B, hssi_b)?;
        self.write8(reg::TX_PAUSE, pause)?;
        self.write8(reg::BEACON_CONTROL_A, beacon)?;
        self.write8(reg::BEACON_CONTROL_B, beacon_b)?;
        self.write32(0x040, gpio)?;
        self.write32(reg::GAIN_CONTROL_A, (gain & !0xff) | 0x50)?;
        self.write32(reg::GAIN_CONTROL_A, gain)?;
        self.write32(reg::TX_TEST_TONE_A, 0x01008c00)?;
        self.write32(reg::RX_TEST_TONE_A, 0x01008c00)?;
        measured
    }

    fn apply_iq(&mut self, values: [u16; 4], receive: ReceiveCorrection) -> Result<(), Error> {
        // Multiply the signed TX coefficients by the existing gain scale.
        // The correction fields are split across imbalance, AFE and ECCA
        // registers; shifts below follow rtwn_iq_calib_write_results.
        let scale = ((self.read32(reg::TX_IQ_IMBALANCE)? >> 22) & 0x3ff) as i32;
        let signed = |value: u16| ((value as i32) << 22) >> 22;
        let x = signed(values[TX_I]) * scale;
        let y = signed(values[TX_Q]) * scale;
        self.update32(reg::TX_IQ_IMBALANCE, 0x3ff, ((x >> 8) as u32) & 0x3ff)?;
        self.update32(
            reg::ECCA_THRESHOLD,
            0x80000000,
            (((x >> 7) as u32) & 1) << 31,
        )?;
        self.update32(
            reg::TX_ANALOG_FRONT_END,
            0xf0000000,
            (((y >> 8) as u32) & 0x3c0) << 22,
        )?;
        self.update32(
            reg::TX_IQ_IMBALANCE,
            0x003f0000,
            (((y >> 8) as u32) & 0x3f) << 16,
        )?;
        self.update32(
            reg::ECCA_THRESHOLD,
            0x20000000,
            (((y >> 7) as u32) & 1) << 29,
        )?;
        if let ReceiveCorrection::KeepDefaults = receive {
            return Ok(());
        }
        self.update32(
            reg::RX_IQ_IMBALANCE,
            0xffff,
            values[RX_I] as u32 | ((values[RX_Q] as u32 & 0x3f) << 10),
        )?;
        self.update32(
            reg::RX_IQ_EXTENSION,
            0xf0000000,
            (values[RX_Q] as u32 & 0x3c0) << 22,
        )
    }
}
