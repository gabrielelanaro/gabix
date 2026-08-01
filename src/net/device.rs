//! Network-device ownership and Ethernet protocol dispatch.

use crate::drivers::FrameDevice;
use crate::net;
use crate::net::arp::{self, ArpPacket, ARP_PACKET_LEN, OPER_REQUEST};
use crate::net::ethernet::{EthernetHeader, ETHERNET_HEADER_LEN, ETHERTYPE_ARP, ETHERTYPE_IPV4};
use crate::net::ipv4::{Ipv4Header, IPV4_HEADER_LEN};
use crate::net::udp::{self, UdpDatagram, UdpHeader, UDP_HEADER_LEN};
use crate::println;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const ETHERNET_MTU: usize = 1500;

/// One Ethernet frame returned by the device-facing network API.
pub struct Frame {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ethertype: u16,
    pub payload: [u8; ETHERNET_MTU],
    pub payload_len: usize,
}

impl Frame {
    fn from_header_and_payload(header: EthernetHeader, payload_src: &[u8]) -> Self {
        let payload_len = payload_src.len();
        assert!(
            payload_len <= ETHERNET_MTU,
            "Ethernet payload {} exceeds MTU {}",
            payload_len,
            ETHERNET_MTU
        );

        let mut payload = [0u8; ETHERNET_MTU];
        payload[..payload_len].copy_from_slice(payload_src);

        Self {
            dst: header.dst,
            src: header.src,
            ethertype: header.ethertype,
            payload,
            payload_len,
        }
    }

    pub fn print_payload_prefix(&self, max_len: usize) {
        let n = core::cmp::min(max_len, self.payload_len);
        let mut offset = 0;
        while offset < n {
            crate::print!("[net]   {:02x}: ", offset);
            let line_end = core::cmp::min(offset + 16, n);
            for byte in &self.payload[offset..line_end] {
                crate::print!("{:02x} ", byte);
            }
            crate::println!();
            offset = line_end;
        }
    }
}

/// Driver-facing network device surface.
pub struct NetDevice {
    inner: Box<dyn FrameDevice>,
}

/// One packet-level event produced by the network RX dispatcher.
pub enum NetRxEvent {
    Arp(ArpPacket),
    Udp(UdpPacket),
    Tcp,
    Ipv4(Ipv4Packet),
    Other,
}

/// An IPv4 header paired with the exact payload bytes that followed it.
///
/// The RX buffer is recycled as soon as a frame is dispatched. Copying the
/// bounded IPv4 payload here gives higher protocols stable bytes to parse.
pub struct Ipv4Packet {
    pub header: Ipv4Header,
    payload: [u8; ETHERNET_MTU - IPV4_HEADER_LEN],
    payload_len: usize,
}

impl Ipv4Packet {
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len]
    }
}

/// A checksum-verified UDP datagram with its IPv4 endpoints.
pub struct UdpPacket {
    pub ip_header: Ipv4Header,
    pub header: UdpHeader,
    payload: [u8; ETHERNET_MTU - IPV4_HEADER_LEN - UDP_HEADER_LEN],
    payload_len: usize,
}

impl UdpPacket {
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.payload_len]
    }
}

lazy_static::lazy_static! {
    static ref NET_DEVICE: spin::Mutex<Option<NetDevice>> = spin::Mutex::new(None);
}

pub fn install_net_device(device: NetDevice) {
    let mac = device.mac();
    *NET_DEVICE.lock() = Some(device);
    println!(
        "[net] device installed mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );
}

pub fn with_net_device<R>(f: impl FnOnce(&mut NetDevice) -> R) -> Option<R> {
    NET_DEVICE.lock().as_mut().map(f)
}

/// Non-spinning variant of [`with_net_device`]: returns `None` if the
/// device is currently owned by another task (a sender mid-transmit).
///
/// The network task uses this because it runs its drain with interrupts
/// disabled (so the empty-check and the block are atomic). A spinning
/// lock there would be fatal: the timer couldn't preempt the spin, the
/// task holding the device would never get the CPU back, and the system
/// would deadlock. Every lock acquisition that *can* spin must happen
/// with interrupts enabled so the timer can always break the cycle.
pub fn try_with_net_device<R>(f: impl FnOnce(&mut NetDevice) -> R) -> Option<R> {
    NET_DEVICE
        .try_lock()
        .and_then(|mut guard| guard.as_mut().map(f))
}

/// PID of the deferred network task, registered at spawn time. The RX
/// interrupt reads this to know which task to wake. `u64::MAX` means "no
/// network task exists". The ISR handles that by skipping the wake.
static NET_TASK_PID: AtomicU64 = AtomicU64::new(u64::MAX);
static NET_TASK_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn register_net_task(pid: crate::process::Pid) {
    NET_TASK_PID.store(pid.0, Ordering::Release);
}

pub fn ensure_network_task() {
    let raw = NET_TASK_PID.load(Ordering::Acquire);
    if raw != u64::MAX {
        NET_TASK_ENABLED.store(true, Ordering::Release);
        crate::task::wake(crate::process::Pid(raw));
    }
}

pub fn net_task_pid() -> Option<crate::process::Pid> {
    if !NET_TASK_ENABLED.load(Ordering::Acquire) {
        return None;
    }
    let raw = NET_TASK_PID.load(Ordering::Acquire);
    if raw == u64::MAX {
        None
    } else {
        Some(crate::process::Pid(raw))
    }
}

/// Deferred network task — Lesson 29's replacement for `network_worker`.
///
/// The old worker looped forever: poll the RX ring, yield, repeat. It
/// burned a schedule slice on every pass even when no frames existed.
/// It drains the RX ring until empty, marks itself Blocked, and sleeps.
/// The RX interrupt can wake it directly; the timer also wakes it as a
/// bounded-latency backstop while virtio-net MSI-X remains unreliable in
/// the current QEMU machine configuration.
///
/// Two rules keep this race-free:
///
/// 1. The drain and the block run with interrupts disabled, so the RX
///    ISR can't fire in between. A frame arriving mid-drain is seen by
///    the drain loop. A frame arriving after the drain pends its MSI in
///    the APIC; the interrupt fires as soon as interrupts re-enable, and
///    `wake` finds the task already Blocked. No lost wakeup.
///
/// 2. The device lock is taken with `try_lock`, never the spinning
///    `lock`. A spinning wait with interrupts disabled would deadlock:
///    the timer couldn't preempt, and whoever holds the device (a task
///    mid-`sendto`) would never run again. On contention the task stays
///    awake and retries next schedule — sends are short, so the retry
///    lands almost immediately.
///
/// This is Linux's NAPI shape: the hardirq handler (our ISR) only
/// schedules the poll; the poll (this task) runs in a context where
/// printing, locking, and transmitting are all safe.
pub unsafe extern "C" fn network_task() {
    println!("[net] deferred RX task started");
    loop {
        crate::arch::interrupts::without_interrupts(|| {
            if try_with_net_device(|device| {
                while device.poll_rx().is_some() {}
                if let Some(outbound) = crate::net::tcp::poll_retransmit(crate::arch::timer_ticks())
                {
                    let _ = device.send_ipv4_packet(
                        crate::net::ipv4::PROTOCOL_TCP,
                        outbound.dst_ip,
                        &outbound.bytes,
                    );
                }
            })
            .is_some()
            {
                crate::task::block_current();
            }
        });
        crate::task::yield_now();
    }
}
impl NetDevice {
    pub fn new(inner: Box<dyn FrameDevice>) -> Self {
        Self { inner }
    }

    pub fn mac(&self) -> [u8; 6] {
        self.inner.mac_address()
    }

    pub fn send_frame(&mut self, eth_frame: &[u8]) {
        self.inner.transmit(eth_frame);
    }

    pub fn recv_frame(&mut self) -> Option<Frame> {
        let raw = self.inner.receive()?;
        let bytes = raw.bytes();
        if bytes.len() < ETHERNET_HEADER_LEN {
            println!("[net] RX dropped short Ethernet frame len={}", bytes.len());
            return None;
        }

        let header = EthernetHeader::parse(&bytes[..ETHERNET_HEADER_LEN])?;
        let payload_len = core::cmp::min(bytes.len() - ETHERNET_HEADER_LEN, ETHERNET_MTU);
        let frame = Frame::from_header_and_payload(
            header,
            &bytes[ETHERNET_HEADER_LEN..ETHERNET_HEADER_LEN + payload_len],
        );

        println!(
            "[net] RX ethertype=0x{:04x} src={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} dst={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} payload_len={}",
            frame.ethertype,
            frame.src[0],
            frame.src[1],
            frame.src[2],
            frame.src[3],
            frame.src[4],
            frame.src[5],
            frame.dst[0],
            frame.dst[1],
            frame.dst[2],
            frame.dst[3],
            frame.dst[4],
            frame.dst[5],
            frame.payload_len
        );
        Some(frame)
    }

    pub fn poll_rx(&mut self) -> Option<NetRxEvent> {
        let frame = self.recv_frame()?;
        Some(self.dispatch_frame(&frame))
    }

    fn dispatch_frame(&mut self, frame: &Frame) -> NetRxEvent {
        if let Some(packet) = self.handle_arp_frame(frame) {
            return NetRxEvent::Arp(packet);
        }

        if let Some(packet) = self.handle_ipv4_frame(frame) {
            if packet.header.protocol == net::ipv4::PROTOCOL_TCP {
                if let Some(reply) =
                    net::tcp::receive(packet.header.src_ip, packet.header.dst_ip, packet.payload())
                {
                    let _ =
                        self.send_ipv4_packet(net::ipv4::PROTOCOL_TCP, reply.dst_ip, &reply.bytes);
                }
                return NetRxEvent::Tcp;
            }
            if packet.header.protocol == net::ipv4::PROTOCOL_UDP {
                let udp_bytes = packet.payload();
                let Some(datagram) =
                    UdpDatagram::parse(udp_bytes, packet.header.src_ip, packet.header.dst_ip)
                else {
                    println!("[udp] dropped malformed datagram");
                    return NetRxEvent::Other;
                };

                println!(
                    "[udp] rx src={}.{}.{}.{}:{} dst={}.{}.{}.{}:{} len={} payload_len={} checksum ok",
                    packet.header.src_ip[0],
                    packet.header.src_ip[1],
                    packet.header.src_ip[2],
                    packet.header.src_ip[3],
                    datagram.header.src_port,
                    packet.header.dst_ip[0],
                    packet.header.dst_ip[1],
                    packet.header.dst_ip[2],
                    packet.header.dst_ip[3],
                    datagram.header.dst_port,
                    datagram.header.length,
                    datagram.payload.len()
                );

                let mut payload = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN - UDP_HEADER_LEN];
                payload[..datagram.payload.len()].copy_from_slice(datagram.payload);
                let udp_packet = UdpPacket {
                    ip_header: packet.header,
                    header: datagram.header,
                    payload,
                    payload_len: datagram.payload.len(),
                };
                crate::net::socket::deliver(
                    udp_packet.header.dst_port,
                    crate::net::socket::Endpoint {
                        ip: udp_packet.ip_header.src_ip,
                        port: udp_packet.header.src_port,
                    },
                    udp_packet.payload(),
                );
                return NetRxEvent::Udp(udp_packet);
            }

            return NetRxEvent::Ipv4(packet);
        }

        NetRxEvent::Other
    }

    /// Resolve `ip` to a MAC address, sleeping until the reply arrives.
    ///
    /// Used where no network task exists (boot, before `network_task` is
    /// spawned), so this function drains the RX ring itself after every
    /// wakeup. The RX MSI-X interrupt — or the 100 Hz timer as a
    /// backstop — ends each `hlt`. Syscall-driven TX paths use
    /// [`resolve_arp_blocking`] instead, which never holds the device
    /// lock while sleeping.
    pub fn resolve_arp(&mut self, ip: [u8; 4]) -> Option<[u8; 6]> {
        if let Some(mac) = arp::lookup(ip) {
            println!(
                "[arp] cache hit: {}.{}.{}.{} is at {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                ip[0], ip[1], ip[2], ip[3], mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            );
            return Some(mac);
        }

        println!(
            "[arp] cache miss: asking for {}.{}.{}.{}",
            ip[0], ip[1], ip[2], ip[3]
        );
        self.send_arp_request(ip);

        for _ in 0..500u32 {
            crate::arch::interrupts::disable();
            // Dispatch whatever arrived, then re-check the cache. The
            // reply we need may already sit in the RX used ring.
            let _ = self.poll_rx();
            if let Some(mac) = arp::lookup(ip) {
                crate::arch::interrupts::enable();
                println!(
                    "[arp] {}.{}.{}.{} is at {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    ip[0], ip[1], ip[2], ip[3], mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
                );
                return Some(mac);
            }
            crate::arch::interrupts::enable_and_hlt();
        }

        println!(
            "[arp] resolve timed out for {}.{}.{}.{}",
            ip[0], ip[1], ip[2], ip[3]
        );
        None
    }

    pub fn send_ipv4_packet(
        &mut self,
        protocol: u8,
        dst_ip: [u8; 4],
        payload: &[u8],
    ) -> Option<()> {
        assert!(
            payload.len() <= ETHERNET_MTU - IPV4_HEADER_LEN,
            "IPv4 payload {} exceeds Ethernet MTU room {}",
            payload.len(),
            ETHERNET_MTU - IPV4_HEADER_LEN
        );

        let broadcast = dst_ip == [255; 4];
        let next_hop_ip = next_hop_for(dst_ip);
        // ARP is resolved before this function is called, either at boot
        // (gateway entry) or by `resolve_arp_blocking` on the syscall
        // path. Sleeping here would deadlock: the network task needs this
        // same device lock to deliver the ARP reply we're waiting for.
        // On a miss, send an ARP request to prime the cache and drop the
        // packet — the caller's retry or the next packet gets through.
        let next_hop_mac = if broadcast {
            [0xff; 6]
        } else if let Some(mac) = arp::lookup(next_hop_ip) {
            mac
        } else {
            println!(
                "[arp] no cache entry for next hop {}.{}.{}.{} — priming and dropping packet",
                next_hop_ip[0], next_hop_ip[1], next_hop_ip[2], next_hop_ip[3]
            );
            self.send_arp_request(next_hop_ip);
            return None;
        };
        let source_ip = net::source_ip();
        let ip_header = Ipv4Header::build(protocol, source_ip, dst_ip, payload.len());

        let ethernet_header = EthernetHeader {
            dst: next_hop_mac,
            src: self.mac(),
            ethertype: ETHERTYPE_IPV4,
        };

        let frame_len = ETHERNET_HEADER_LEN + IPV4_HEADER_LEN + payload.len();
        let mut frame = [0u8; ETHERNET_HEADER_LEN + ETHERNET_MTU];
        frame[0..ETHERNET_HEADER_LEN].copy_from_slice(&ethernet_header.to_bytes());
        frame[ETHERNET_HEADER_LEN..ETHERNET_HEADER_LEN + IPV4_HEADER_LEN]
            .copy_from_slice(&ip_header);
        frame[ETHERNET_HEADER_LEN + IPV4_HEADER_LEN..frame_len].copy_from_slice(payload);

        self.send_frame(&frame[..frame_len]);
        println!(
            "[ip] tx proto={} src={}.{}.{}.{} dst={}.{}.{}.{} len={} next-hop={}.{}.{}.{} ({:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
            protocol,
            source_ip[0], source_ip[1], source_ip[2], source_ip[3],
            dst_ip[0],
            dst_ip[1],
            dst_ip[2],
            dst_ip[3],
            IPV4_HEADER_LEN + payload.len(),
            next_hop_ip[0],
            next_hop_ip[1],
            next_hop_ip[2],
            next_hop_ip[3],
            next_hop_mac[0],
            next_hop_mac[1],
            next_hop_mac[2],
            next_hop_mac[3],
            next_hop_mac[4],
            next_hop_mac[5]
        );

        Some(())
    }

    /// Build and send one UDP datagram through the IPv4 and Ethernet paths.
    pub fn send_udp(
        &mut self,
        dst_ip: [u8; 4],
        dst_port: u16,
        src_port: u16,
        payload: &[u8],
    ) -> Option<()> {
        let mut datagram = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN];
        let datagram_len = udp::write_datagram(
            &mut datagram,
            net::source_ip(),
            dst_ip,
            src_port,
            dst_port,
            payload,
        )?;
        self.send_ipv4_packet(net::ipv4::PROTOCOL_UDP, dst_ip, &datagram[..datagram_len])
    }

    /// Send one ARP request for `target_ip`. Public so the blocking
    /// resolver can prime the cache without owning the device.
    pub fn send_arp_request(&mut self, target_ip: [u8; 4]) {
        let packet = ArpPacket::request(self.mac(), net::source_ip(), target_ip);
        self.send_arp_packet([0xff; 6], packet);
    }

    fn send_arp_reply(&mut self, request: &ArpPacket) {
        let reply = ArpPacket::reply_to(request, self.mac(), net::source_ip());
        self.send_arp_packet(request.sha, reply);
        println!(
            "[arp] replied: {}.{}.{}.{} is at {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            net::source_ip()[0],
            net::source_ip()[1],
            net::source_ip()[2],
            net::source_ip()[3],
            self.mac()[0],
            self.mac()[1],
            self.mac()[2],
            self.mac()[3],
            self.mac()[4],
            self.mac()[5]
        );
    }

    fn send_arp_packet(&mut self, dst_mac: [u8; 6], packet: ArpPacket) {
        let header = EthernetHeader {
            dst: dst_mac,
            src: self.mac(),
            ethertype: ETHERTYPE_ARP,
        };
        let mut frame = [0u8; ETHERNET_HEADER_LEN + ARP_PACKET_LEN];
        frame[0..ETHERNET_HEADER_LEN].copy_from_slice(&header.to_bytes());
        frame[ETHERNET_HEADER_LEN..].copy_from_slice(&packet.to_bytes());

        self.send_frame(&frame);
        println!("[net] TX broadcast frame sent, {} bytes", frame.len());
    }

    fn handle_arp_frame(&mut self, frame: &Frame) -> Option<ArpPacket> {
        if frame.ethertype != ETHERTYPE_ARP {
            return None;
        }

        let packet = ArpPacket::parse(&frame.payload[..frame.payload_len])?;
        if !packet.is_ipv4_over_ethernet() {
            println!("[arp] dropped unsupported ARP packet");
            return None;
        }

        arp::merge(&packet);
        println!(
            "[arp] learned {}.{}.{}.{} -> {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            packet.spa[0],
            packet.spa[1],
            packet.spa[2],
            packet.spa[3],
            packet.sha[0],
            packet.sha[1],
            packet.sha[2],
            packet.sha[3],
            packet.sha[4],
            packet.sha[5]
        );

        if packet.oper == OPER_REQUEST
            && net::config()
                .map(|c| packet.tpa == c.address)
                .unwrap_or(false)
        {
            self.send_arp_reply(&packet);
        }

        Some(packet)
    }

    pub fn handle_ipv4_frame(&mut self, frame: &Frame) -> Option<Ipv4Packet> {
        if frame.ethertype != ETHERTYPE_IPV4 {
            return None;
        }

        let header = Ipv4Header::parse(&frame.payload[..frame.payload_len])?;
        if !net::accepts_destination(header.dst_ip) {
            println!(
                "[ip] dropped frame for {}.{}.{}.{}",
                header.dst_ip[0], header.dst_ip[1], header.dst_ip[2], header.dst_ip[3]
            );
            return None;
        }

        println!(
            "[ip] rx proto={} src={}.{}.{}.{} dst={}.{}.{}.{} len={}",
            header.protocol,
            header.src_ip[0],
            header.src_ip[1],
            header.src_ip[2],
            header.src_ip[3],
            header.dst_ip[0],
            header.dst_ip[1],
            header.dst_ip[2],
            header.dst_ip[3],
            header.total_len
        );

        let payload_len = header.payload_len();
        let ip_payload = &frame.payload[IPV4_HEADER_LEN..IPV4_HEADER_LEN + payload_len];

        if header.protocol == net::ipv4::PROTOCOL_ICMP {
            if let Some(request) = net::icmp::EchoMessage::parse(ip_payload) {
                if request.message_type == net::icmp::ECHO_REQUEST
                    && request.code == net::icmp::ECHO_CODE
                {
                    let mut reply = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN];
                    let reply_len = net::icmp::write_echo_reply(
                        &mut reply,
                        request.identifier,
                        request.sequence,
                        request.payload,
                    )
                    .expect("maximum IPv4 payload fits in an Ethernet frame");

                    if self
                        .send_ipv4_packet(
                            net::ipv4::PROTOCOL_ICMP,
                            header.src_ip,
                            &reply[..reply_len],
                        )
                        .is_some()
                    {
                        println!(
                            "[icmp] echoed request id=0x{:04x} seq={} to {}.{}.{}.{}",
                            request.identifier,
                            request.sequence,
                            header.src_ip[0],
                            header.src_ip[1],
                            header.src_ip[2],
                            header.src_ip[3]
                        );
                    }
                }
            }
        }

        let mut payload = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN];
        payload[..payload_len].copy_from_slice(ip_payload);

        Some(Ipv4Packet {
            header,
            payload,
            payload_len,
        })
    }

    fn handle_udp_packet(&mut self, packet: &UdpPacket) {
        if packet.header.dst_port != udp::ECHO_PORT {
            println!(
                "[udp] no receiver for destination port {}",
                packet.header.dst_port
            );
            return;
        }

        if self
            .send_udp(
                packet.ip_header.src_ip,
                packet.header.src_port,
                udp::ECHO_PORT,
                packet.payload(),
            )
            .is_some()
        {
            println!(
                "[udp] echoed port {} payload_len={} to {}.{}.{}.{}:{}",
                udp::ECHO_PORT,
                packet.payload().len(),
                packet.ip_header.src_ip[0],
                packet.ip_header.src_ip[1],
                packet.ip_header.src_ip[2],
                packet.ip_header.src_ip[3],
                packet.header.src_port
            );
            println!(
                "[udp] *** guest port {} echoed one datagram ***",
                udp::ECHO_PORT
            );
        } else {
            println!("[udp] echo transmit failed");
        }
    }
}

fn next_hop_for(dst_ip: [u8; 4]) -> [u8; 4] {
    let config = net::config();
    if let Some(value) = config {
        if dst_ip[0] & value.subnet_mask[0] == value.address[0] & value.subnet_mask[0]
            && dst_ip[1] & value.subnet_mask[1] == value.address[1] & value.subnet_mask[1]
            && dst_ip[2] & value.subnet_mask[2] == value.address[2] & value.subnet_mask[2]
            && dst_ip[3] & value.subnet_mask[3] == value.address[3] & value.subnet_mask[3]
        {
            dst_ip
        } else {
            value.gateway
        }
    } else {
        dst_ip
    }
}

/// Resolve the next hop for `dst_ip`, sleeping until the ARP reply
/// arrives. For syscall TX paths (`sendto`): the device lock is held
/// only for the request transmit, never while sleeping, so the network
/// task can deliver the reply into the ARP cache.
pub fn resolve_next_hop(dst_ip: [u8; 4]) -> Option<[u8; 6]> {
    resolve_arp_blocking(next_hop_for(dst_ip))
}

/// Blocking ARP resolve that never holds the NET_DEVICE lock while
/// sleeping. Sends one request, then `hlt`-sleeps until the network task
/// (woken by the RX interrupt) delivers the reply into the cache. The
/// 100 Hz timer is the backstop that re-checks the cache at least every
/// 10 ms even if an interrupt is somehow missed.
fn resolve_arp_blocking(ip: [u8; 4]) -> Option<[u8; 6]> {
    if let Some(mac) = arp::lookup(ip) {
        return Some(mac);
    }

    println!(
        "[arp] syscall path miss: asking for {}.{}.{}.{}",
        ip[0], ip[1], ip[2], ip[3]
    );
    let _ = with_net_device(|device| device.send_arp_request(ip));

    for _ in 0..500u32 {
        crate::arch::interrupts::disable();
        if let Some(mac) = arp::lookup(ip) {
            crate::arch::interrupts::enable();
            println!(
                "[arp] {}.{}.{}.{} is at {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                ip[0], ip[1], ip[2], ip[3], mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            );
            return Some(mac);
        }
        crate::arch::interrupts::enable_and_hlt();
    }

    println!(
        "[arp] syscall path resolve timed out for {}.{}.{}.{}",
        ip[0], ip[1], ip[2], ip[3]
    );
    None
}
