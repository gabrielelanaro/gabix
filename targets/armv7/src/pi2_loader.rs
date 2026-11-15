#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
#[cfg(feature = "pi-sd-loader")]
#[path = "raspi2b/mailbox.rs"]
mod mailbox;
#[cfg(feature = "pi-sd-loader")]
#[path = "raspi2b/reset.rs"]
mod reset;
#[cfg(feature = "pi-sd-loader")]
#[path = "raspi2b/sdhci.rs"]
mod sdhci;

// The SD adapter shares kernel-core's device interface; this loader never allocates.
#[cfg(feature = "pi-sd-loader")]
struct NoHeap;
#[cfg(feature = "pi-sd-loader")]
unsafe impl core::alloc::GlobalAlloc for NoHeap {
    unsafe fn alloc(&self, _: core::alloc::Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: core::alloc::Layout) {}
}
#[cfg(feature = "pi-sd-loader")]
#[global_allocator]
static NO_HEAP: NoHeap = NoHeap;

use gabix_kernel::loader_protocol::{UploadEvent, UploadFailure, UploadReceiver};

core::arch::global_asm!(include_str!("pi2_loader.S"));

const UART_BASE: usize = 0x3f20_1000;
const UART_DATA: usize = 0x00;
const UART_FLAGS: usize = 0x18;
const UART_RX_EMPTY: u32 = 1 << 4;
const UART_TX_FULL: u32 = 1 << 5;
const UART_BUSY: u32 = 1 << 3;

const SYSTEM_TIMER_LOW: usize = 0x3f00_3004;
const UPLOAD_TIMEOUT_MICROSECONDS: u32 = 5_000_000;
const DEVELOPMENT_KERNEL_ADDRESS: usize = 0x0010_0000;
#[cfg(feature = "pi-sd-loader")]
const UART_OVERRIDE_WINDOW_MICROSECONDS: u32 = 2_000_000;
#[cfg(feature = "pi-sd-loader")]
const BOOT_WATCHDOG_SECONDS: u32 = 15;
#[cfg(feature = "pi-sd-loader")]
const SD_SECTOR_BYTES: u32 = 512;
#[cfg(feature = "pi-sd-loader")]
const WATCHDOG_REFRESH_BYTES: u32 = 32 * SD_SECTOR_BYTES;

#[no_mangle]
extern "C" fn pi2_loader_main() -> ! {
    clear_boot_handoff();
    let uart = Uart;
    let mut receiver = UploadReceiver::new();
    uart.write(b"GABIX LOADER READY\r\n");
    #[cfg(feature = "pi-sd-loader")]
    {
        // Starting a UART upload within this window takes priority over SD boot.
        if let Some(byte) = uart.read_with_timeout(UART_OVERRIDE_WINDOW_MICROSECONDS) {
            handle_event(&uart, receiver.push(byte));
        } else {
            try_sd_boot(&uart);
            uart.write(b"GABIX LOADER READY\r\n");
        }
    }

    loop {
        let byte = if receiver.is_receiving_payload() {
            match uart.read_with_timeout(UPLOAD_TIMEOUT_MICROSECONDS) {
                Some(byte) => byte,
                None => {
                    handle_event(&uart, receiver.timeout());
                    continue;
                }
            }
        } else {
            uart.read_blocking()
        };

        handle_event(&uart, receiver.push(byte));
    }
}

fn handle_event(uart: &Uart, event: UploadEvent) {
    match event {
        UploadEvent::Waiting => {}
        UploadEvent::Payload { offset, byte } => write_kernel_byte(offset, byte),
        UploadEvent::Complete { offset, byte, .. } => {
            write_kernel_byte(offset, byte);
            uart.write(b"UPLOAD CRC OK\r\n");
            uart.write(b"UPLOAD OK\r\n");
            uart.write(b"HANDOFF ADDRESS ");
            uart.write_hex32(DEVELOPMENT_KERNEL_ADDRESS as u32);
            uart.write(b"\r\nHANDOFF LENGTH ");
            uart.write_hex32(offset + 1);
            uart.write(b"\r\nHANDOFF FIRST WORD ");
            uart.write_hex32(unsafe { read32(DEVELOPMENT_KERNEL_ADDRESS) });
            uart.write(b"\r\nHANDOFF SCTLR ");
            uart.write_hex32(read_sctlr());
            uart.write(b"\r\nHANDOFF CPSR ");
            uart.write_hex32(read_cpsr());
            uart.write(b"\r\nHANDOFF BRANCH\r\n");
            uart.flush();
            clear_boot_handoff();
            call_development_kernel();
            uart.write(b"\r\nHANDOFF RETURNED\r\n");
            uart.write(b"GABIX LOADER READY\r\n");
            uart.flush();
        }
        UploadEvent::Failed { reason } => {
            match reason {
                UploadFailure::InvalidHeader => uart.write(b"UPLOAD HEADER FAILED\r\n"),
                UploadFailure::CrcMismatch => uart.write(b"UPLOAD CRC FAILED\r\n"),
                UploadFailure::Timeout => uart.write(b"UPLOAD TIMEOUT\r\n"),
            }
            uart.write(b"UPLOAD FAILED\r\n");
            uart.write(b"GABIX LOADER READY\r\n");
        }
    }
}

#[cfg(feature = "pi-sd-loader")]
// Read/validate the partition and boot record, then try its selected copies.
// Record I/O failure returns to UART immediately; an image read/CRC failure
// advances to the next copy. Confirmation is done later by the running kernel.
fn try_sd_boot(uart: &Uart) {
    use gabix_kernel::boot_record::{find_partition, update_crc, BootRecord};
    use gabix_kernel::device::WritableBlockDevice;
    use gabix_kernel::loader_protocol::MAX_KERNEL_SIZE;
    reset::arm_deadman(BOOT_WATCHDOG_SECONDS);
    let outcome = (|| unsafe {
        let mut card = sdhci::SdCard::initialize()?;
        let mbr = card.try_read_sector(0)?;
        let (partition_start, partition_sectors) = find_partition(&mbr, card.sector_count())?;
        let bytes = card.try_read_sector(partition_start)?;
        let mut record = BootRecord::decode(&bytes, partition_sectors)?;
        while let Some(index) = record.next_attempt() {
            // Persist the attempt before loading or executing anything. If the
            // watchdog resets us, the next boot skips this unconfirmed copy.
            let bytes = record.encode();
            card.write_sector(partition_start, &bytes).ok()?;
            if card.try_read_sector(partition_start)? != bytes {
                return None;
            }
            uart.write(b"GABIX SD TRY ");
            uart.write_hex32(index as u32);
            uart.write(b"\r\n");
            let selected = record.copies[index];
            if selected.image_bytes > MAX_KERNEL_SIZE {
                continue;
            }
            let start = partition_start + selected.offset_sectors;
            // Stream sectors straight into the kernel destination. A bad CRC
            // leaves those bytes in RAM, but execution is allowed only on a match.
            let loaded = (|| {
                let mut crc = !0; // CRC32 initial state; complement after the final byte.
                for offset in (0..selected.image_bytes).step_by(SD_SECTOR_BYTES as usize) {
                    if offset % WATCHDOG_REFRESH_BYTES == 0 {
                        reset::arm_deadman(BOOT_WATCHDOG_SECONDS);
                    }
                    let sector = card.try_read_sector(start + offset as u64 / SD_SECTOR_BYTES as u64)?;
                    let count = (selected.image_bytes - offset).min(SD_SECTOR_BYTES) as usize;
                    crc = update_crc(crc, &sector[..count]);
                    for (i, &byte) in sector[..count].iter().enumerate() {
                        write_kernel_byte(offset + i as u32, byte);
                    }
                }
                (!crc == selected.image_crc32).then_some(())
            })();
            if loaded.is_none() {
                uart.write(b"GABIX SD COPY INVALID\r\n");
                continue;
            }
            write_boot_handoff(
                partition_start,
                index as u32,
                selected.image_bytes,
                selected.image_crc32,
            );
            uart.write(b"GABIX SD KERNEL VERIFIED\r\n");
            uart.flush();
            // Discard cached instructions before branching to the verified image.
            let zero = 0u32;
            asm!("dsb sy", "mcr p15, 0, {zero}, c7, c5, 0", "isb", zero = in(reg) zero);
            reset::arm_deadman(BOOT_WATCHDOG_SECONDS);
            call_development_kernel();
            // Returning is treated as failure. This path assumes the callee
            // preserved the loader's stack/mapping environment on return.
            clear_boot_handoff();
        }
        uart.write(b"GABIX SD COPIES EXHAUSTED\r\n");
        Some(())
    })();
    reset::stop_deadman();
    clear_boot_handoff();
    if outcome.is_none() {
        uart.write(b"GABIX SD BOOT UNAVAILABLE; UART READY\r\n");
    }
}

#[cfg(feature = "pi-sd-loader")]
unsafe fn write_boot_handoff(record_sector: u64, index: u32, length: u32, crc: u32) {
    use gabix_kernel::boot_record::{crc32, handoff_field, HANDOFF_ADDRESS, HANDOFF_MAGIC};
    let mut bytes = [0u8; handoff_field::BYTE_LEN];
    bytes[handoff_field::MAGIC].copy_from_slice(&HANDOFF_MAGIC.to_le_bytes());
    bytes[handoff_field::RECORD_SECTOR].copy_from_slice(&record_sector.to_le_bytes());
    bytes[handoff_field::COPY_INDEX].copy_from_slice(&index.to_le_bytes());
    bytes[handoff_field::IMAGE_LENGTH].copy_from_slice(&length.to_le_bytes());
    bytes[handoff_field::IMAGE_CRC].copy_from_slice(&crc.to_le_bytes());
    let checksum = crc32(&bytes[handoff_field::CHECKSUM_INPUT]);
    bytes[handoff_field::CHECKSUM].copy_from_slice(&checksum.to_le_bytes());
    for (offset, byte) in bytes.into_iter().enumerate() {
        core::ptr::write_volatile((HANDOFF_ADDRESS + offset) as *mut u8, byte);
    }
}

fn clear_boot_handoff() {
    unsafe {
        core::ptr::write_volatile(gabix_kernel::boot_record::HANDOFF_ADDRESS as *mut u32, 0);
    }
}

// The upload decoder or decoded boot record must bound offset before this call.
// This raw store performs no independent destination-range check.
fn write_kernel_byte(offset: u32, byte: u8) {
    unsafe {
        core::ptr::write_volatile(
            (DEVELOPMENT_KERNEL_ADDRESS + offset as usize) as *mut u8,
            byte,
        );
    }
}

fn call_development_kernel() {
    unsafe {
        asm!(
            "blx {entry}",
            entry = in(reg) DEVELOPMENT_KERNEL_ADDRESS,
            clobber_abi("C"),
        );
    }
}

fn read_sctlr() -> u32 {
    let value: u32;
    unsafe {
        asm!(
            "mrc p15, 0, {value}, c1, c0, 0",
            value = out(reg) value,
            options(nostack, preserves_flags)
        );
    }
    value
}

fn read_cpsr() -> u32 {
    let value: u32;
    unsafe {
        asm!(
            "mrs {value}, cpsr",
            value = out(reg) value,
            options(nostack, preserves_flags)
        );
    }
    value
}

struct Uart;

impl Uart {
    fn read_blocking(&self) -> u8 {
        loop {
            if let Some(byte) = self.try_read() {
                return byte;
            }
            core::hint::spin_loop();
        }
    }

    // Busy-poll the timer's wrapping microsecond counter. The small configured
    // deadlines fit within one wrap; receive gets priority at the deadline.
    fn read_with_timeout(&self, timeout_microseconds: u32) -> Option<u8> {
        let started = timer_low();
        loop {
            if let Some(byte) = self.try_read() {
                return Some(byte);
            }
            if timer_low().wrapping_sub(started) >= timeout_microseconds {
                return None;
            }
            core::hint::spin_loop();
        }
    }

    // Consume the low data byte; UART receive-error flags are discarded.
    fn try_read(&self) -> Option<u8> {
        unsafe {
            if read32(UART_BASE + UART_FLAGS) & UART_RX_EMPTY != 0 {
                None
            } else {
                Some(read32(UART_BASE + UART_DATA) as u8)
            }
        }
    }

    // Wait for FIFO space per byte, with no local timeout.
    fn write(&self, bytes: &[u8]) {
        for &byte in bytes {
            unsafe {
                while read32(UART_BASE + UART_FLAGS) & UART_TX_FULL != 0 {
                    core::hint::spin_loop();
                }
                write32(UART_BASE + UART_DATA, byte as u32);
            }
        }
    }

    fn write_hex32(&self, value: u32) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        self.write(b"0x");
        for shift in (0..8).rev() {
            self.write(&[HEX[((value >> (shift * 4)) & 0xf) as usize]]);
        }
    }

    // Wait for transmission to finish, with no local timeout.
    fn flush(&self) {
        unsafe {
            while read32(UART_BASE + UART_FLAGS) & UART_BUSY != 0 {
                core::hint::spin_loop();
            }
        }
    }
}

fn timer_low() -> u32 {
    unsafe { read32(SYSTEM_TIMER_LOW) }
}

unsafe fn read32(address: usize) -> u32 {
    core::ptr::read_volatile(address as *const u32)
}

unsafe fn write32(address: usize, value: u32) {
    core::ptr::write_volatile(address as *mut u32, value);
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
