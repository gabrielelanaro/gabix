#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod allocator;
mod console;
mod context;
mod memory;
mod network;
mod platform;
#[cfg(feature = "raspi2b")]
mod raspi2b;
mod scheduler;
#[cfg(feature = "pi-sd-transfer")]
mod sd_transfer;
#[cfg(feature = "pi-sd-transfer")]
mod sd_install;
#[cfg(feature = "pi-sd-transfer")]
mod diagnostics;
#[cfg(feature = "pi-managed")]
mod management_auth;
#[cfg(feature = "pi-sd-transfer")]
mod boot_trial;
#[cfg(feature = "pi-sd-transfer")]
static mut PI_SD_TRANSFER: Option<sd_transfer::Receiver> = None;
mod process;
mod syscall;
mod user;
#[cfg(feature = "pi-uart-user")]
mod user_upload;
#[cfg(feature = "virt")]
mod virt;

use console::write_line;
use console::Pl011;
use core::alloc::Layout;
use core::arch::asm;
use core::arch::global_asm;
use core::panic::PanicInfo;
use gabix_kernel::device::FrameDevice;
use network::{DhcpSession, NetworkEvent};
use platform::{PLATFORM, UART_BASE};

#[cfg(any(
    feature = "pi-hardware",
    feature = "pi-lifecycle",
    feature = "pi-uart-user"
))]
const RASPI2B_SVC_STACK_TOP: u32 = 0x0800_0000;

#[cfg(any(
    feature = "pi-hardware",
    feature = "pi-lifecycle",
    feature = "pi-uart-user"
))]
const SEMIHOSTING: u32 = 0;
#[cfg(not(any(
    feature = "pi-hardware",
    feature = "pi-lifecycle",
    feature = "pi-uart-user"
)))]
const SEMIHOSTING: u32 = 1;

#[cfg(any(feature = "pi-hardware", feature = "pi-lifecycle", feature = "pi-uart-user"))]
static mut PI_NETWORK: Option<raspi2b::ethernet::UsbEthernet> = None;
#[cfg(any(feature = "pi-hardware", feature = "pi-lifecycle", feature = "pi-uart-user"))]
static mut PI_DHCP: Option<DhcpSession> = None;

#[cfg(all(feature = "virt", feature = "raspi2b"))]
compile_error!("select one ARM platform");

#[cfg(not(any(feature = "virt", feature = "raspi2b")))]
compile_error!("select an ARM platform");

#[cfg(feature = "virt")]
global_asm!(
    include_str!("boot.S"),
    uart_base = const 0x0900_0000,
    gpio_base = const 0,
    configure_gpio = const 0,
    stack_top = const 0x47ff_0000,
    irq_stack_top = const 0x47fe_0000,
    rust_entry = sym arm_kernel_entry,
    irq_entry = sym arm_irq_entry,
    svc_entry = sym arm_svc_entry,
    memory_abort_entry = sym arm_memory_abort_entry,
    semihosting = const SEMIHOSTING,
);

#[cfg(feature = "raspi2b")]
global_asm!(
    include_str!("boot.S"),
    uart_base = const 0x3f20_1000,
    gpio_base = const 0x3f20_0000,
    configure_gpio = const 1,
    stack_top = const 0x0800_0000,
    irq_stack_top = const 0x07ff_0000,
    rust_entry = sym arm_kernel_entry,
    irq_entry = sym arm_irq_entry,
    svc_entry = sym arm_svc_entry,
    memory_abort_entry = sym arm_memory_abort_entry,
    semihosting = const SEMIHOSTING,
);

#[no_mangle]
extern "C" fn arm_kernel_entry() {
    #[cfg(feature = "pi-sd-transfer")]
    unsafe {
        boot_trial::begin();
    }
    let mut console = Pl011::new(UART_BASE);
    gabix_kernel::announce_boot(&mut console, PLATFORM);

    #[cfg(any(
        feature = "pi-network-dev",
        feature = "pi-network-pre-timer-probe",
        feature = "pi-network-no-irq-dev"
    ))]
    {
        raspi2b::reset::arm_deadman(15);
        write_line(&mut console, b"gabix raspi2b development watchdog armed");
    }

    #[cfg(feature = "raspi2b")]
    unsafe {
        if memory::enable_smp() {
            write_line(&mut console, b"gabix raspi2b smp enabled");
        } else {
            write_line(&mut console, b"gabix raspi2b smp unavailable");
        }
        if memory::enable_identity_mapping() {
            write_line(&mut console, b"gabix raspi2b memory mapped");
        }
        allocator::init();
        #[cfg(feature = "pi-sd-transfer")]
        {
            raspi2b::reset::arm_deadman(15);
            let receiver = sd_transfer::Receiver::initialize();
            let ready = receiver.is_some();
            PI_SD_TRANSFER = receiver;
            release_boot_test_watchdog();
            if ready {
                write_line(&mut console, b"gabix SD transfer region ready");
                flush_diagnostics(true);
            } else {
                write_line(&mut console, b"gabix SD transfer unavailable: card or region check failed");
            }
        }
    }

    #[cfg(feature = "virt")]
    unsafe {
        allocator::init();
    }

    #[cfg(feature = "virt")]
    unsafe {
        let mut disk_user_entry = None;
        if memory::frame_is_reused() {
            write_line(&mut console, b"gabix arm virt frame reused");
        }
        if memory::enable_identity_mapping() {
            write_line(&mut console, b"gabix arm virt memory mapped");
        }
        let boxed = alloc::boxed::Box::new(40u32);
        let mut values = alloc::vec::Vec::new();
        values.extend_from_slice(&[1u32, 1]);
        if *boxed + values.iter().sum::<u32>() == 42 {
            write_line(&mut console, b"gabix arm virt heap allocated");
        }
        if scheduler::task_resumes() {
            write_line(&mut console, b"gabix arm virt task resumed");
        }
        virt::timer::init();
        verify_timer_preemption(&mut console);
        verify_process_isolation(&mut console);
        if let Some(mut block) = virt::block::VirtioBlock::probe() {
            if let Some(sector) = block.try_read_sector(1) {
                if sector.starts_with(b"GABIXARM") {
                    write_line(&mut console, b"gabix arm virt block read");
                }
            }
            if let Some(file) = gabix_kernel::fs::ext2::read_file(&mut block, b"/known-block.bin") {
                if file.starts_with(b"gabix-known-disk-data\n") {
                    write_line(&mut console, b"gabix arm virt file read");
                }
            }
            if let Some(image) = gabix_kernel::fs::ext2::read_file(&mut block, b"/init.elf") {
                disk_user_entry = user::load_elf(&image);
            }
        }
        if let Some(mut network) = virt::network::VirtioNet::probe() {
            let mut session = DhcpSession::new(network.mac_address(), 0x4741_4258);
            let mut frame = [0u8; 29];
            frame[..6].fill(0xff);
            frame[6..12].copy_from_slice(&network.mac_address());
            frame[12..14].copy_from_slice(&0x88b5u16.to_be_bytes());
            frame[14..].copy_from_slice(b"gabix-arm-frame");
            network.transmit(&frame);
            session.send_discover(&mut network);
            let deadline = virt::timer::ticks() + 10;
            while virt::timer::ticks() < deadline {
                if let Some(received) = network.receive() {
                    let bytes = received.bytes();
                    if bytes.len() >= 30 && bytes[14..].starts_with(b"gabix-host-frame") {
                        write_line(&mut console, b"gabix arm virt frame received");
                    }
                    if matches!(
                        session.handle_frame(&mut network, bytes),
                        NetworkEvent::Configured
                    ) {
                        write_line(&mut console, b"gabix arm virt dhcp configured");
                    }
                }
                asm!("wfi");
            }
        }
        while virt::timer::ticks() < 2 {
            asm!("wfi");
        }
        write_line(&mut console, b"gabix arm virt timer advanced");

        if let Some(entry) = disk_user_entry.or_else(user::loaded_entry) {
            memory::allow_user_section(entry);
            memory::allow_user_section(user::USER_STACK_TOP - 1);
            user::enter(entry, user::USER_STACK_TOP);
        }
    }

    #[cfg(feature = "raspi2b")]
    unsafe {
        #[cfg(not(feature = "pi-uart-user"))]
        let mut disk_user_entry = None;
        let boxed = alloc::boxed::Box::new(40u32);
        let values = alloc::vec![1u32, 1];
        if *boxed + values.iter().sum::<u32>() == 42 {
            write_line(&mut console, b"gabix raspi2b heap allocated");
        }
        #[cfg(feature = "pi-hardware")]
        raspi2b::reset::arm_deadman(15);
        if scheduler::task_resumes() {
            // Persistent builds release the boot-test watchdog. Diagnostic
            // builds keep their watchdog armed through device initialization.
            #[cfg(all(feature = "pi-hardware", not(any(
                feature = "pi-network-dev",
                feature = "pi-network-pre-timer-probe",
                feature = "pi-network-no-irq-dev"
            ))))]
            release_boot_test_watchdog();
            write_line(&mut console, b"gabix arm scheduler trace 10 20 11 21 12 22");
            write_line(&mut console, b"gabix arm scheduler tasks finished");
        }
        #[cfg(not(feature = "pi-uart-user"))]
        if let Some(mut card) = raspi2b::sdhci::SdCard::initialize() {
            if let Some(sector) = card.try_read_sector(1) {
                if sector.starts_with(b"GABIXPI2") {
                    write_line(&mut console, b"gabix raspi2b block read");
                }
            }
            let root_start =
                gabix_kernel::fs::partition::mbr_partition_start(&mut card, 0x83).unwrap_or(0);
            let mut root =
                gabix_kernel::fs::partition::OffsetBlockDevice::new(&mut card, root_start);
            if let Some(file) = gabix_kernel::fs::ext2::read_file(&mut root, b"/known-block.bin") {
                if file.starts_with(b"gabix-known-disk-data\n") {
                    write_line(&mut console, b"gabix raspi2b file read");
                }
            }
            #[cfg(not(feature = "pi-uart-user"))]
            if let Some(image) = gabix_kernel::fs::ext2::read_file(&mut root, b"/init.elf") {
                disk_user_entry = user::load_elf(&image);
            }
        }
        #[cfg(not(any(
            feature = "pi-network-pre-timer-probe",
            feature = "pi-network-no-irq-dev"
        )))]
        {
            raspi2b::timer::init();
            #[cfg(feature = "pi-sd-transfer")]
            flush_diagnostics(true);
            verify_timer_preemption(&mut console);
            #[cfg(feature = "pi-sd-transfer")]
            flush_diagnostics(true);
            verify_process_isolation(&mut console);
            #[cfg(feature = "pi-sd-transfer")]
            flush_diagnostics(true);
        }
        #[cfg(any(not(feature = "pi-uart-user"), feature = "pi-hardware"))]
        if let Some(mut usb) = raspi2b::usb::Dwc2::initialize() {
            if let Some(device) = usb.enumerate_root() {
                write_line(&mut console, b"gabix raspi2b usb enumerated");
                let network_device = if device.device_class == 9 {
                    #[cfg(feature = "pi-wifi")]
                    {
                        let mut wireless = None;
                        let mut wired = None;
                        let _ = usb.visit_hub_children(device, |_, child| {
                            if (child.vendor_id, child.product_id) == (0x0bda, 0x8176) {
                                wireless = Some(child);
                            } else if gabix_kernel::net::smsc95xx::is_lan9512(child.vendor_id, child.product_id) {
                                wired = Some(child);
                            }
                            true
                        });
                        wireless.or(wired)
                    }
                    #[cfg(not(feature = "pi-wifi"))]
                    usb.enumerate_hub_child(device)
                } else {
                    Some(device)
                };
                if let Some(mut network) = network_device
                    .and_then(|device| raspi2b::ethernet::UsbEthernet::configure(usb, device))
                {
                    write_line(&mut console, b"gabix raspi2b usb network configured");
                    #[cfg(feature = "pi-hardware")]
                    if !network.is_wifi() {
                        match network.link_up() {
                            Some(true) => write_line(&mut console, b"gabix raspi2b ethernet link up"),
                            Some(false) => write_line(&mut console, b"gabix raspi2b ethernet link down"),
                            None => write_line(&mut console, b"gabix raspi2b ethernet link unknown"),
                        }
                        write_line(&mut console, b"gabix raspi2b waiting for ethernet link");
                        raspi2b::timer::delay_us(5_000_000);
                        match network.link_up() {
                            Some(true) => write_line(&mut console, b"gabix raspi2b ethernet link up"),
                            Some(false) => write_line(&mut console, b"gabix raspi2b ethernet link down"),
                            None => write_line(&mut console, b"gabix raspi2b ethernet link unknown"),
                        }
                    }
                    #[cfg(feature = "pi-network-pre-timer-probe")]
                    {
                        let session = DhcpSession::new(network.mac_address(), 0x4741_4258);
                        let mut transmitted = 0usize;
                        for _ in 0..16 {
                            if session.send_discover(&mut network) {
                                transmitted += 1;
                            }
                        }
                        if transmitted == 16 {
                            write_line(&mut console, b"gabix raspi2b pre-timer dhcp tx 16");
                        } else {
                            write_line(&mut console, b"gabix raspi2b pre-timer dhcp tx incomplete");
                        }
                        loop {
                            core::hint::spin_loop();
                        }
                    }
                    let mut session = DhcpSession::new(network.mac_address(), 0x4741_4258);
                    #[cfg(not(feature = "pi-managed"))]
                    let mut next_discover = raspi2b::timer::ticks();
                    #[cfg(all(feature = "pi-hardware", not(feature = "pi-managed")))]
                    let mut discover_count = 0usize;
                    #[cfg(not(feature = "pi-hardware"))]
                    let deadline = raspi2b::timer::ticks() + 200;
                    #[cfg(feature = "pi-hardware")]
                    let deadline = raspi2b::timer::ticks() + 2_000;
                    while raspi2b::timer::ticks() < deadline {
                        #[cfg(not(feature = "pi-managed"))]
                        let now = raspi2b::timer::ticks();
                        #[cfg(feature = "pi-managed")]
                        {
                            let link = network.link_up() != Some(false);
                            session.poll(&mut network, raspi2b::timer::uptime_millis()/1000, link);
                            record_network_progress(&session);
                            boot_trial::refresh();
                            flush_diagnostics(false);
                        }
                        #[cfg(not(feature = "pi-managed"))]
                        if !session.is_configured() && now >= next_discover {
                            write_line(&mut console, b"gabix raspi2b dhcp discover");
                            if session.send_discover(&mut network) {
                                write_line(&mut console, b"gabix raspi2b dhcp tx ok");
                            } else {
                                write_line(&mut console, b"gabix raspi2b dhcp tx failed");
                            }
                            #[cfg(feature = "pi-hardware")]
                            {
                                discover_count += 1;
                            }
                            #[cfg(feature = "pi-sd-transfer")]
                            boot_trial::refresh();
                            next_discover = now + 100;
                        }
                        #[cfg(all(feature = "pi-hardware", not(feature = "pi-managed")))]
                        let poll_receive = discover_count >= 2;
                        #[cfg(feature = "pi-managed")]
                        let poll_receive = true;
                        #[cfg(not(feature = "pi-hardware"))]
                        let poll_receive = true;
                        if poll_receive {
                            if let Some(frame) = network.receive() {
                                match session.handle_frame(&mut network, frame.bytes()) {
                                    NetworkEvent::Configured => {
                                        write_line(&mut console, b"gabix raspi2b dhcp configured");
                                        #[cfg(any(feature = "pi-network-dev", feature = "pi-network-no-irq-dev"))]
                                        raspi2b::reset::arm_deadman(15);
                                        #[cfg(feature = "pi-uart-user")]
                                        break;
                                    }
                                    NetworkEvent::ReplyTransmitted(true) => {
                                        write_line(&mut console, b"gabix raspi2b network reply tx ok");
                                    }
                                    NetworkEvent::ReplyTransmitted(false) => {
                                        write_line(&mut console, b"gabix raspi2b network reply tx failed");
                                    }
                                    NetworkEvent::None => {}
                                }
                            }
                        }
                        #[cfg(feature = "pi-network-no-irq-dev")]
                        core::hint::spin_loop();
                        #[cfg(all(feature = "pi-wifi", feature = "pi-managed"))]
                        if let Some(event) = network.take_wifi_event() {
                            diagnostics::event(format_args!("wifi {event:?}"));
                        }
                        #[cfg(not(feature = "pi-network-no-irq-dev"))]
                        asm!("wfi");
                    }
                    #[cfg(any(feature = "pi-hardware", feature = "pi-lifecycle", feature = "pi-uart-user"))]
                    {
                        PI_NETWORK = Some(network);
                        PI_DHCP = Some(session);
                    }
                } else {
                    write_line(&mut console, b"gabix ethernet device configuration FAILED");
                }
            } else {
                write_line(&mut console, b"gabix USB root enumeration FAILED");
            }
        } else {
            write_line(&mut console, b"gabix USB host initialization FAILED");
        }
        while raspi2b::timer::ticks() < 2 {
            #[cfg(feature = "pi-network-no-irq-dev")]
            core::hint::spin_loop();
            #[cfg(not(feature = "pi-network-no-irq-dev"))]
            asm!("wfi");
        }
        write_line(&mut console, b"gabix raspi2b timer advanced");
        #[cfg(feature = "pi-uart-user")]
        user_upload::serve_uart_uploads_and_ethernet(&mut console);
        #[cfg(not(feature = "pi-uart-user"))]
        if let Some(entry) = disk_user_entry {
            memory::allow_user_section(entry);
            memory::allow_user_section(user::USER_STACK_TOP - 1);
            user::enter(entry, user::USER_STACK_TOP);
        }
    }
}

/// Poll from the UART loader's idle loop; the IRQ handler never owns these objects.
#[cfg(feature = "pi-uart-user")]
pub fn poll_ethernet_and_reply(console: &mut Pl011, _next_dhcp_discover_tick: &mut u64) {
    unsafe {
        let network = &mut *core::ptr::addr_of_mut!(PI_NETWORK);
        let session = &mut *core::ptr::addr_of_mut!(PI_DHCP);
        if let (Some(network), Some(session)) = (network.as_mut(), session.as_mut()) {
            #[cfg(not(feature = "pi-managed"))]
            let current_tick = raspi2b::timer::ticks();
            #[cfg(feature = "pi-managed")]
            {
                let now = raspi2b::timer::uptime_millis();
                if now >= NEXT_LINK_CHECK {
                    let link = match network.link_up() {
                        Some(true) => 2,
                        Some(false) => 1,
                        None => 0,
                    };
                    if link != LINK_STATUS {
                        diagnostics::event(format_args!(
                            "network link {}",
                            match link {
                                2 => "up",
                                1 => "down",
                                _ => "unknown",
                            }
                        ));
                        if LINK_STATUS != 0 {
                            session.restart("link changed");
                        }
                        LINK_STATUS = link;
                        flush_diagnostics(true);
                    }
                    NEXT_LINK_CHECK = now + 2000;
                }
                session.poll(network, now / 1000, LINK_STATUS != 1);
                record_network_progress(session);
                if let Some(receiver) = (&mut *core::ptr::addr_of_mut!(PI_SD_TRANSFER)).as_mut() {
                    receiver.network_status(session, LINK_STATUS);
                }
            }
            #[cfg(not(feature = "pi-managed"))]
            if !session.is_configured() && current_tick >= *_next_dhcp_discover_tick {
                session.send_discover(network);
                *_next_dhcp_discover_tick = current_tick + 100;
            }
            if let Some(frame) = network.receive() {
                #[cfg(feature = "pi-sd-transfer")]
                if let Some(receiver) = (&mut *core::ptr::addr_of_mut!(PI_SD_TRANSFER)).as_mut() {
                    if session.handle_sd_transfer(network, frame.bytes(), receiver) {
                        return;
                    }
                }
                if matches!(
                    session.handle_frame(network, frame.bytes()),
                    NetworkEvent::Configured
                ) {
                    write_line(console, b"gabix raspi2b dhcp configured");
                }
            }
            #[cfg(all(feature = "pi-wifi", feature = "pi-managed"))]
            if let Some(event) = network.take_wifi_event() {
                if matches!(event, raspi2b::wifi_ethernet::Event::State(raspi2b::wifi_ethernet::State::Online)) {
                    session.restart("wifi associated");
                }
                diagnostics::event(format_args!("wifi {event:?}"));
            }
        }
    }
}

#[cfg(any(
    feature = "pi-hardware",
    feature = "pi-lifecycle",
    feature = "pi-uart-user"
))]
#[unsafe(naked)]
extern "C" fn arm_kernel_after_user_trampoline() -> ! {
    core::arch::naked_asm!(
        "mov r0, #{stack_top}",
        "mov sp, r0",
        "b {after_user}",
        stack_top = const RASPI2B_SVC_STACK_TOP,
        after_user = sym arm_kernel_after_user,
    );
}

#[cfg(any(
    feature = "pi-hardware",
    feature = "pi-lifecycle",
    feature = "pi-uart-user"
))]
#[no_mangle]
extern "C" fn arm_kernel_after_user() -> ! {
    let mut console = Pl011::new(UART_BASE);
    write_line(&mut console, b"gabix raspi2b userspace exited");
    #[cfg(feature = "pi-uart-user")]
    user_upload::serve_uart_uploads_and_ethernet(&mut console);
    #[cfg(not(feature = "pi-uart-user"))]
    let mut next_discover = raspi2b::timer::ticks();
    #[cfg(not(feature = "pi-uart-user"))]
    unsafe {
        loop {
            let network = &mut *core::ptr::addr_of_mut!(PI_NETWORK);
            let session = &mut *core::ptr::addr_of_mut!(PI_DHCP);
            if let (Some(network), Some(session)) = (network.as_mut(), session.as_mut()) {
                let now = raspi2b::timer::ticks();
                if !session.is_configured() && now >= next_discover {
                    session.send_discover(network);
                    next_discover = now + 500;
                }
                if let Some(frame) = network.receive() {
                    if matches!(
                        session.handle_frame(network, frame.bytes()),
                        NetworkEvent::Configured
                    ) {
                        write_line(&mut console, b"gabix raspi2b dhcp configured");
                    }
                }
            }
            asm!("wfi");
        }
    }
}

#[alloc_error_handler]
fn allocation_error(_: Layout) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

unsafe fn verify_process_isolation(console: &mut Pl011) {
    #[cfg(feature = "pi-hardware")]
    raspi2b::reset::arm_deadman(15);
    if process::verify_isolation() {
        write_line(console, b"gabix arm isolated userspace processes passed");
        write_line(console, b"gabix arm user faults contained; peer process survived");
        #[cfg(all(feature = "pi-hardware", not(any(feature = "pi-network-dev", feature = "pi-network-pre-timer-probe", feature = "pi-network-no-irq-dev"))))]
        release_boot_test_watchdog();
    } else {
        write_line(console, b"gabix arm process isolation FAILED");
        #[cfg(feature = "pi-sd-transfer")]
        if boot_trial::is_active() {
            raspi2b::reset::reset();
        }
    }
}

#[no_mangle]
unsafe extern "C" fn arm_memory_abort_entry(address: u32, status: u32, saved_status: u32, instruction: u32) -> ! {
    process::handle_memory_abort(address, status, saved_status, instruction != 0)
}

/// The physical watchdog returns a stalled test to the UART loader.
unsafe fn verify_timer_preemption(console: &mut Pl011) {
    #[cfg(feature = "pi-hardware")]
    raspi2b::reset::arm_deadman(15);
    if scheduler::timer_preempts_tasks() {
        write_line(console, b"gabix arm timer preempted two non-yielding tasks");
        #[cfg(all(feature = "pi-hardware", not(any(
            feature = "pi-network-dev", feature = "pi-network-pre-timer-probe",
            feature = "pi-network-no-irq-dev"
        ))))]
        release_boot_test_watchdog();
    } else {
        write_line(console, b"gabix arm preemption check failed");
        #[cfg(feature = "pi-sd-transfer")]
        if boot_trial::is_active() {
            raspi2b::reset::reset();
        }
    }
}

#[no_mangle]
extern "C" fn arm_irq_entry() {
    #[cfg(feature = "virt")]
    unsafe {
        if virt::timer::handle_interrupt() { scheduler::preempt_on_timer(); }
    }
    #[cfg(feature = "raspi2b")]
    unsafe {
        raspi2b::timer::handle_interrupt();
        scheduler::preempt_on_timer();
    }
}

#[no_mangle]
extern "C" fn arm_svc_entry(frame: *mut syscall::UserFrame) {
    unsafe {
        syscall::dispatch(&mut *frame);
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "pi-hardware")]
fn release_boot_test_watchdog() {
    #[cfg(feature = "pi-sd-transfer")]
    if boot_trial::is_active() {
        boot_trial::refresh();
        return;
    }
    raspi2b::reset::stop_deadman();
}
#[cfg(feature = "pi-sd-transfer")]
pub fn confirm_sd_boot(console: &mut Pl011) {
    if !boot_trial::is_active() {
        return;
    }
    unsafe {
        let network_ready = (&*core::ptr::addr_of!(PI_NETWORK)).is_some();
        if network_ready {
            if let Some(receiver) = (&mut *core::ptr::addr_of_mut!(PI_SD_TRANSFER)).as_mut() {
                if receiver.confirm_boot() {
                    write_line(console, b"GABIX SD BOOT CONFIRMED");
                    return;
                }
            }
        }
    }
    write_line(console, b"GABIX SD BOOT UNCONFIRMED; watchdog will retry");
}

#[cfg(feature = "pi-sd-transfer")]
pub fn flush_diagnostics(critical: bool) {
    unsafe {
        if let Some(receiver) = (&mut *core::ptr::addr_of_mut!(PI_SD_TRANSFER)).as_mut() {
            receiver.maintenance(critical);
        }
    }
}
#[cfg(feature = "pi-managed")]
static mut LINK_STATUS: u32 = 0;
#[cfg(feature = "pi-managed")]
static mut NEXT_LINK_CHECK: u64 = 0;
#[cfg(feature = "pi-managed")]
fn record_network_progress(session: &DhcpSession) {
    static mut PREVIOUS: (u32, u32, u32, u64) = (u32::MAX, 0, 0, 0);
    let state = session.state as u32;
    let remaining = session.lease_remaining();
    unsafe {
        let previous = &mut *core::ptr::addr_of_mut!(PREVIOUS);
        let attempt_summary = session.attempts != previous.1
            && (session.attempts.is_power_of_two() || session.attempts % 64 == 0);
        if state != previous.0 || remaining > previous.3.saturating_add(2) {
            let label = match session.state {
                network::LeaseState::Selecting => "discovering",
                network::LeaseState::Requesting => "requesting",
                network::LeaseState::Bound => "bound",
                network::LeaseState::Renewing => "renewing",
                network::LeaseState::Rebinding => "rebinding",
            };
            diagnostics::event(format_args!(
                "dhcp {} ip={:?} attempts={} lease_remaining_s={} server={:?} discovery_reason={}",
                label,
                session.address(),
                session.attempts,
                remaining,
                session.lease.map(|l| l.server).unwrap_or([0; 4]),
                session.discovery_reason
            ));
            flush_diagnostics(true);
        } else if attempt_summary || session.tx_failures != previous.2 {
            diagnostics::event(format_args!(
                "dhcp attempts={} transmit_failures={}",
                session.attempts, session.tx_failures
            ));
        }
        *previous = (state, session.attempts, session.tx_failures, remaining);
    }
}
