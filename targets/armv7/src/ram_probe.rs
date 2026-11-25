//! Single-owner returning ARM programs, available only through the authenticated
//! development build. Programs are trusted kernel code and must obey AAPCS.
use crate::raspi2b::reset;
use core::{arch::asm, ptr::addr_of_mut};

const BEGIN: u32 = 20;
const WRITE: u32 = 21;
const RUN: u32 = 22;
const READ_RESULT: u32 = 23;
const INVALID: u32 = 1;
const ORDER: u32 = 2;
const CHECKSUM: u32 = 4;
const IMAGE_BYTES: usize = 256 * 1024;
const OUTPUT_BYTES: usize = 32 * 1024;

#[repr(C, align(4096))]
struct Aligned<const N: usize>([u8; N]);
#[link_section = ".probe_ram"]
static mut IMAGE: Aligned<IMAGE_BYTES> = Aligned([0; IMAGE_BYTES]);
static mut OUTPUT: Aligned<OUTPUT_BYTES> = Aligned([0; OUTPUT_BYTES]);

pub struct Runner {
    session: u32,
    length: usize,
    received: usize,
    crc: u32,
    verified: bool,
    result_len: usize,
}

impl Runner {
    pub const fn new() -> Self {
        Self {
            session: 0,
            length: 0,
            received: 0,
            crc: 0,
            verified: false,
            result_len: 0,
        }
    }

    // Called synchronously by the sole network receiver after HMAC verification.
    // The management layer caches duplicate packets, including RUN acknowledgements.
    pub fn command(
        &mut self,
        op: u32,
        session: u32,
        arg: u32,
        value: u32,
        data: &[u8],
        reply: &mut [u8],
    ) -> Option<Result<usize, u32>> {
        if !(BEGIN..=READ_RESULT).contains(&op) {
            return None;
        }
        let offset = arg as usize;
        let image = unsafe { &mut (*addr_of_mut!(IMAGE)).0 };
        Some(match op {
            BEGIN if session != 0 && data.is_empty() && (4..=IMAGE_BYTES).contains(&offset) => {
                image.fill(0); // Raw images omit BSS. Each new load starts with zeroed workspace.
                *self = Self {
                    session,
                    length: offset,
                    crc: value,
                    ..Self::new()
                };
                Ok(0)
            }
            _ if session == 0 || session != self.session => Err(ORDER),
            WRITE
                if !data.is_empty()
                    && data.len() <= 512
                    && offset == self.received
                    && offset + data.len() <= self.length =>
            {
                image[offset..offset + data.len()].copy_from_slice(data);
                self.received += data.len();
                Ok(0)
            }
            RUN if self.received == self.length && data.len() <= 512 => {
                if !self.verified
                    && gabix_kernel::boot_record::crc32(&image[..self.length]) != self.crc
                {
                    Err(CHECKSUM)
                } else {
                    self.verified = true;
                    self.result_len = 0;
                    let output = unsafe { &mut (*addr_of_mut!(OUTPUT)).0 };
                    output.fill(0);
                    // Rust drops the image slice borrow before the external call.
                    let address = image.as_mut_ptr();
                    unsafe {
                        synchronize_code(address, self.length);
                    }
                    type Entry = unsafe extern "C" fn(*const u8, usize, *mut u8, usize) -> i32;
                    let entry: Entry = unsafe { core::mem::transmute(address) };
                    reset::arm_deadman(10);
                    let count = unsafe {
                        entry(data.as_ptr(), data.len(), output.as_mut_ptr(), output.len())
                    };
                    reset::arm_deadman(15);
                    if count > OUTPUT_BYTES as i32 {
                        Err(INVALID)
                    } else {
                        self.result_len = count.max(0) as usize;
                        reply[..4].copy_from_slice(&count.to_le_bytes());
                        Ok(4)
                    }
                }
            }
            READ_RESULT if data.is_empty() && offset < self.result_len => {
                let count = (self.result_len - offset).min(reply.len());
                let output = unsafe { &(*addr_of_mut!(OUTPUT)).0 };
                reply[..count].copy_from_slice(&output[offset..offset + count]);
                Ok(count)
            }
            _ => Err(INVALID),
        })
    }
}

// The CPU wrote new instructions via its data cache. Clean to RAM, discard old
// I-cache and branch predictions, then synchronize instruction fetch on core 0.
unsafe fn synchronize_code(base: *mut u8, size: usize) {
    for offset in (0..size).step_by(32) {
        asm!("mcr p15, 0, {a}, c7, c10, 1", a = in(reg) base.add(offset));
    }
    asm!("dsb sy", "mcr p15, 0, {z}, c7, c5, 0", "mcr p15, 0, {z}, c7, c5, 6", "dsb sy", "isb", z = in(reg) 0u32);
}
