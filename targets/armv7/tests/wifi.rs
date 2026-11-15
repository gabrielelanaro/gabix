#![no_std]
#![no_main]
//! Physical Wi-Fi bring-up check: runs in RAM and returns to SD boot by watchdog.
#[path = "../src/raspi2b/mailbox.rs"]
mod mailbox;
#[path = "../src/raspi2b/rtl8188cu.rs"]
mod rtl8188cu;
#[path = "../src/raspi2b/usb.rs"]
mod usb;
use core::fmt::{self, Write};
core::arch::global_asm!(include_str!("wifi_boot.S"));
struct Console;
impl Write for Console {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            unsafe {
                while core::ptr::read_volatile(0x3f201018 as *const u32) & 32 != 0 {}
                core::ptr::write_volatile(0x3f201000 as *mut u32, byte as u32);
            }
        }
        Ok(())
    }
}
#[no_mangle]
extern "C" fn wifi_check_main() -> ! {
    let _ = writeln!(Console, "WIFI CHECK START");
    match check() {
        Ok(()) => {
            let _ = writeln!(Console, "WIFI CHECK PASS");
        }
        Err(error) => {
            let _ = writeln!(
                Console,
                "WIFI CHECK FAILED {error:?} uptime_us={}",
                unsafe { core::ptr::read_volatile(0x3f003004 as *const u32) }
            );
        }
    }
    loop {
        core::hint::spin_loop();
    }
}
fn check() -> Result<(), rtl8188cu::Error> {
    use rtl8188cu::Error;
    let mut usb = unsafe { usb::Dwc2::initialize() }.ok_or(Error::Configuration)?;
    let _ = writeln!(
        Console,
        "WIFI FIFO RX={:08x} NPTX={:08x} PTX={:08x} HW3={:08x} DFIFO={:08x}",
        unsafe { core::ptr::read_volatile(0x3f980024 as *const u32) },
        unsafe { core::ptr::read_volatile(0x3f980028 as *const u32) },
        unsafe { core::ptr::read_volatile(0x3f980100 as *const u32) },
        unsafe { core::ptr::read_volatile(0x3f98004c as *const u32) },
        unsafe { core::ptr::read_volatile(0x3f98005c as *const u32) }
    );
    let root = unsafe { usb.enumerate_root() }.ok_or(Error::Configuration)?;
    let mut wifi = None;
    unsafe {
        usb.visit_hub_children(root, |port, child| {
            let _ = writeln!(
                Console,
                "USB port={port} id={:04x}:{:04x}",
                child.vendor_id, child.product_id
            );
            if (child.vendor_id, child.product_id) == (0x0bda, 0x8176) {
                wifi = Some(child);
            }
            true
        })
    }
    .map_err(|_| Error::Configuration)?;
    let device = wifi.ok_or(Error::WrongDevice)?;
    #[cfg(feature = "pi-wifi")]
    {
        let network = unsafe {
            wifi_ethernet::WifiEthernet::configure(
                usb,
                device,
                env!("GABIX_WIFI_SSID").as_bytes(),
                include_bytes!(env!("GABIX_WIFI_PMK_FILE")),
                include_bytes!(env!("GABIX_WIFI_FIRMWARE")),
            )
        }?;
        return dhcp_check(network);
    }
    #[cfg(not(feature = "pi-wifi"))]
    {
        let mut radio = unsafe { rtl8188cu::Rtl8188cu::attach(usb, device) }?;
        let _ = writeln!(
            Console,
            "WIFI endpoints in={} out={} packet={}",
            radio.endpoints.input, radio.endpoints.output, radio.endpoints.max_packet_size
        );
        let identity = radio.identify()?;
        let _ = writeln!(
            Console,
            "WIFI identity SYS_CFG={:08x} MAC={:02x?} PA={:02x} board={:02x}",
            identity.system_config, identity.mac, identity.pa_setting, identity.rom[0x79]
        );
        if option_env!("GABIX_WIFI_STAGE") == Some("registers") {
            radio.verify_identity_reads(identity.system_config)?;
            let _ = writeln!(Console, "WIFI 4096 REGISTER READS MATCHED");
            return Ok(());
        }
        radio.power_off()?;
        let _ = writeln!(Console, "WIFI WARM STATE RESET");
        radio.power_on()?;
        let _ = writeln!(Console, "WIFI MAC POWERED uptime_us={}", unsafe {
            core::ptr::read_volatile(0x3f003004 as *const u32)
        });
        let scan = matches!(
            option_env!("GABIX_WIFI_STAGE"),
            Some("scan" | "join" | "wpa")
        );
        if scan {
            radio.initialize_packet_memory()?;
        }
        let firmware = include_bytes!(env!("GABIX_WIFI_FIRMWARE"));
        radio.load_firmware(firmware)?;
        let _ = writeln!(Console, "WIFI FIRMWARE READY");
        if scan {
            radio.initialize_receiver(&identity)?;
            let _ = writeln!(Console, "WIFI RECEIVER READY");
            let _ = writeln!(
                Console,
                "WIFI IQ TX calibrated; RX correction={}",
                radio.rx_iq_calibrated
            );
            let (bssid, channel, capabilities) = passive_scan(&mut radio)?;
            if matches!(option_env!("GABIX_WIFI_STAGE"), Some("join" | "wpa")) {
                radio.prepare_station(&identity, bssid, channel)?;
                radio.clear_keys()?;
                join(&mut radio, identity.mac, bssid, capabilities)?;
            }
        }
        Ok(())
    }
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Console, "WIFI PANIC {info}");
    loop {
        core::hint::spin_loop();
    }
}

#[repr(align(64))]
struct ReceiveBuffer([u8; 4096]);
fn passive_scan(radio: &mut rtl8188cu::Rtl8188cu) -> Result<([u8; 6], u8, u16), rtl8188cu::Error> {
    let target = option_env!("GABIX_WIFI_SSID").unwrap_or("");
    let mut buffer = ReceiveBuffer([0; 4096]);
    let mut count = 0u32;

    for channel in (1..=13).cycle().take(39) {
        unsafe {
            core::ptr::write_volatile(0x3f100024 as *mut u32, 0x5a0f0000);
        }
        radio.set_channel(channel)?;
        let timer = 0x3f003004 as *const u32;
        let start = unsafe { core::ptr::read_volatile(timer) };
        while unsafe { core::ptr::read_volatile(timer) }.wrapping_sub(start) < 500_000 {
            let Some(length) = radio.receive(&mut buffer.0) else {
                continue;
            };
            if length < 24 {
                continue;
            }
            let status = u32::from_le_bytes(buffer.0[..4].try_into().unwrap());
            let packet_length = (status & 0x3fff) as usize;
            let offset = 24 + (((status >> 16) & 15) as usize) * 8 + ((status >> 24) & 3) as usize;
            if status & 0xc000 != 0 || offset + packet_length > length {
                continue;
            }
            let frame = &buffer.0[offset..offset + packet_length];
            if let Some(beacon) = gabix_kernel::net::wifi::parse_beacon(frame) {
                count += 1;
                if beacon.ssid == target.as_bytes() {
                    let _ = writeln!(
                        Console,
                        "WIFI TARGET channel={:?} bssid={:02x?} rsn={:02x?}",
                        beacon.channel, beacon.bssid, beacon.rsn
                    );
                    return Ok((
                        beacon.bssid,
                        beacon.channel.unwrap_or(channel),
                        beacon.capabilities,
                    ));
                }
            }
        }
        let _ = writeln!(Console, "WIFI SCAN channel={channel} beacons={count}");
    }
    Err(rtl8188cu::Error::Configuration)
}

// The packet parsers allocate nothing. kernel-core also exposes allocating
// interfaces, so the standalone hardware check explicitly rejects allocation.
struct NoHeap;
unsafe impl core::alloc::GlobalAlloc for NoHeap {
    unsafe fn alloc(&self, _: core::alloc::Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: core::alloc::Layout) {}
}
#[global_allocator]
static NO_HEAP: NoHeap = NoHeap;

fn join(
    radio: &mut rtl8188cu::Rtl8188cu,
    station: [u8; 6],
    bssid: [u8; 6],
    capabilities: u16,
) -> Result<(), rtl8188cu::Error> {
    use gabix_kernel::net::wifi;
    let mut request = [0u8; 256];
    let mut receive = ReceiveBuffer([0; 4096]);
    for subtype in [0xb0, 0x10] {
        let length = if subtype == 0xb0 {
            wifi::authentication_request(&mut request, station, bssid, 1)
        } else {
            wifi::association_request(
                &mut request,
                station,
                bssid,
                2,
                option_env!("GABIX_WIFI_SSID").unwrap_or("").as_bytes(),
                capabilities,
            )
        }
        .ok_or(rtl8188cu::Error::Configuration)?;
        let mut accepted = false;
        for _ in 0..3 {
            unsafe {
                core::ptr::write_volatile(0x3f100024 as *mut u32, 0x5a0f0000);
            }
            radio.transmit(&request[..length])?;
            let _ = writeln!(Console, "WIFI JOIN request={subtype:02x} sent");
            let start = unsafe { core::ptr::read_volatile(0x3f003004 as *const u32) };
            while unsafe { core::ptr::read_volatile(0x3f003004 as *const u32) }.wrapping_sub(start)
                < 1_000_000
            {
                let Some(length) = radio.receive(&mut receive.0) else {
                    continue;
                };
                if length < 24 {
                    continue;
                }
                let status = u32::from_le_bytes(receive.0[..4].try_into().unwrap());
                let size = (status & 0x3fff) as usize;
                let offset =
                    24 + (((status >> 16) & 15) as usize) * 8 + ((status >> 24) & 3) as usize;
                if status & 0xc000 != 0 || offset + size > length {
                    continue;
                }
                let frame = &receive.0[offset..offset + size];
                if let Some(status) = wifi::management_reply_status(frame, subtype, station, bssid)
                {
                    let _ = writeln!(Console, "WIFI JOIN reply={subtype:02x} status={status}");
                    if status != 0 {
                        return Err(rtl8188cu::Error::Configuration);
                    }
                    accepted = true;
                    break;
                }
            }
            if accepted {
                break;
            }
        }
        if !accepted {
            return Err(rtl8188cu::Error::Configuration);
        }
    }
    let _ = writeln!(
        Console,
        "WIFI ASSOCIATED; WPA2 authentication still required"
    );
    Ok(())
}

#[cfg(feature = "pi-wifi")]
#[path = "../src/raspi2b/random.rs"]
mod random;

#[cfg(feature = "pi-wifi")]
#[path = "../src/network.rs"]
mod network;
#[cfg(feature = "pi-wifi")]
#[path = "../src/raspi2b/wifi_ethernet.rs"]
mod wifi_ethernet;

#[cfg(feature = "pi-wifi")]
fn dhcp_check(mut device: wifi_ethernet::WifiEthernet) -> Result<(), rtl8188cu::Error> {
    use gabix_kernel::device::FrameDevice;
    let mut session = network::DhcpSession::new(device.mac_address(), 0x57494649);
    let start = unsafe { core::ptr::read_volatile(0x3f003004 as *const u32) };
    let mut configured_at = None;
    let mut previous_link = false;
    let mut reconnected = false;
    loop {
        let elapsed = unsafe { core::ptr::read_volatile(0x3f003004 as *const u32) }
            .wrapping_sub(start)
            / 1000;
        if elapsed > 60_000 {
            return Err(rtl8188cu::Error::Configuration);
        }
        unsafe {
            core::ptr::write_volatile(0x3f100024 as *mut u32, 0x5a0f0000);
        }
        let link = device.link_up();
        if link != previous_link {
            session.restart("Wi-Fi link changed");
            previous_link = link;
        }
        session.poll(&mut device, elapsed as u64 / 1000, link);
        if let Some(frame) = device.receive() {
            if let network::NetworkEvent::Configured =
                session.handle_frame(&mut device, frame.bytes())
            {
                let _ = writeln!(Console, "WIFI DHCP address={:?}", session.address());
                configured_at = Some(elapsed);
            }
        }
        if let Some(event) = device.take_event() {
            let _ = writeln!(Console, "WIFI STATE {event:?}");
        }
        if !reconnected && configured_at.is_some_and(|at| elapsed - at > 5_000) {
            let _ = writeln!(Console, "WIFI RECONNECT requested");
            device.reconnect();
            session.restart("physical reconnect check");
            configured_at = None;
            reconnected = true;
        }
        if device.connections >= 2 && configured_at.is_some_and(|at| elapsed - at > 15_000) {
            let _ = writeln!(
                Console,
                "WIFI RECONNECT verified connections={}",
                device.connections
            );
            return Ok(());
        }
    }
}
