#![no_std]
#![no_main]

#[path = "../src/allocator.rs"]
mod allocator;
#[path = "../src/console.rs"]
mod console;
#[path = "../src/raspi2b/mailbox.rs"]
mod mailbox;
#[path = "../src/raspi2b/reset.rs"]
mod reset;
#[path = "../src/raspi2b/sdhci.rs"]
mod sdhci;
use console::{write_line, Pl011};
use gabix_kernel::BootConsole;

core::arch::global_asm!(
    include_str!("../src/boot.S"),
    uart_base = const 0x3f20_1000, gpio_base = const 0x3f20_0000,
    configure_gpio = const 1, stack_top = const 0x0800_0000,
    irq_stack_top = const 0x07ff_0000, rust_entry = sym check,
    irq_entry = sym fault, svc_entry = sym fault, memory_abort_entry = sym fault,
    semihosting = const 0,
);
fn hex(console: &mut Pl011, value: u32) {
    for shift in (0..8).rev() {
        console.write_byte(b"0123456789abcdef"[((value >> (shift * 4)) & 15) as usize]);
    }
    console.write_byte(b' ');
}
extern "C" fn check() {
    let mut console = Pl011::new(0x3f20_1000);
    reset::arm_deadman(15);
    write_line(&mut console, b"SD check: initialize");
    if let Some(mut card) = unsafe { sdhci::SdCard::initialize() } {
        write_line(&mut console, b"SD check: initialized; reading MBR");
        if let Some(sector) = unsafe { card.try_read_sector(0) } {
            write_line(&mut console, b"SD check: MBR read");
            // This opt-in names the recovery image whose empty pre-partition
            // gap was checked against the saved original image. Never infer
            // that an arbitrary unpartitioned sector is disposable.
            let expected_partitions: [u8; 64] = [
                0, 0, 1, 128, 12, 3, 224, 255, 0, 64, 0, 0, 0, 0, 16, 0, 0, 3, 224, 255, 131, 3,
                224, 255, 0, 64, 16, 0, 0, 64, 58, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ];
            if option_env!("GABIX_SD_CHECK_WRITE") != Some("recovery-image-gap") {
                write_line(&mut console, b"SD check: read-only; write opt-in absent");
            } else if sector[440..444] != [0x8d, 0x2d, 0x1b, 0x41]
                || sector[446..510] != expected_partitions
                || sector[510..] != [0x55, 0xaa]
            {
                write_line(&mut console, b"SD check: unexpected card; no writes");
            } else {
                use gabix_kernel::device::WritableBlockDevice;
                let lba = 8192;
                let original = unsafe { card.try_read_sector(lba) }.unwrap();
                let before = unsafe { card.try_read_sector(lba - 1) }.unwrap();
                let after = unsafe { card.try_read_sector(lba + 1) }.unwrap();
                assert!(original == [0; 512] && before == [0; 512] && after == [0; 512]);
                let mut pattern = [0; 512];
                for (i, byte) in pattern.iter_mut().enumerate() {
                    *byte = (i as u8).wrapping_mul(37).wrapping_add(11);
                }
                let written = card.write_sector(lba, &pattern);
                match written {
                    Ok(()) => write_line(&mut console, b"SD check: write completed"),
                    Err(e) => {
                        write_line(&mut console, b"SD check: write error");
                        hex(
                            &mut console,
                            match e {
                                gabix_kernel::device::BlockError::Controller(v)
                                | gabix_kernel::device::BlockError::Card(v) => v,
                                _ => 0,
                            },
                        );
                    }
                }
                let matches = unsafe { card.try_read_sector(lba) } == Some(pattern);
                let restored = card.write_sector(lba, &original).is_ok()
                    && unsafe { card.try_read_sector(lba) } == Some(original);
                let guards = unsafe { card.try_read_sector(lba - 1) } == Some(before)
                    && unsafe { card.try_read_sector(lba + 1) } == Some(after);
                if written.is_ok() && matches && restored && guards {
                    write_line(&mut console, b"SD check: PASS write/read/restore/neighbors");
                } else {
                    write_line(&mut console, b"SD check: FAIL");
                }
            }
        } else {
            write_line(&mut console, b"SD check: read failed");
        }
    } else {
        write_line(&mut console, b"SD check: initialization failed");
    }
    drain_uart();
    reset::reset();
}
extern "C" fn fault() {
    reset::reset();
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    let mut console = Pl011::new(0x3f20_1000);
    write_line(&mut console, b"SD check: FAIL (precondition or exception)");
    drain_uart();
    reset::reset()
}

fn drain_uart() {
    // Wait until the final result has left the UART before resetting the board.
    unsafe { while core::ptr::read_volatile(0x3f20_1018 as *const u32) & (1 << 3) != 0 {} }
}
