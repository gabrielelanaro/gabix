//! One passive IPv4 stream for the single foreground ELF. All access runs on
//! core 0 in foreground/SVC context. IRQ handlers never enter the network stack.
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use gabix_kernel::device::FrameDevice;
use gabix_kernel::net::ethernet::{EthernetHeader, ETHERNET_HEADER_LEN, ETHERTYPE_IPV4};
use gabix_kernel::net::ipv4::{Ipv4Header, IPV4_HEADER_LEN};
use gabix_kernel::net::stack::MAX_FRAME_LEN;
use gabix_kernel::net::tcp::*;

// The foreground ELF has one listener and one accepted stream. These use the
// existing Gabix socket syscall numbers and sockaddr_in layout.
const SYS_READ: u32 = 0;
const SYS_WRITE: u32 = 1;
const SYS_CLOSE: u32 = 3;
const SYS_SOCKET: u32 = 41;
const SYS_ACCEPT: u32 = 43;
const SYS_BIND: u32 = 49;
const SYS_LISTEN: u32 = 50;
const AF_INET: u32 = 2;
const SOCK_STREAM: u32 = 1;
const LISTENER: u32 = 3;
const STREAM: u32 = 4;
const ERROR: u32 = u32::MAX;
const RX_CAPACITY: usize = 4096;
const MAX_SEND: usize = 536; // Default IPv4 TCP MSS without negotiated options.
const RETRY_MS: u64 = 1000;
const RETRY_LIMIT: u8 = 8;
const TCP_OFFSET: usize = ETHERNET_HEADER_LEN + IPV4_HEADER_LEN;

struct PendingSegment {
    sequence: u32,
    flags: u8,
    bytes: Vec<u8>,
    due: u64,
    attempts: u8,
}

struct Peer {
    endpoints: Endpoints,
    mac: [u8; 6],
    send_next: u32,
    recv_next: u32,
    window: usize,
    flight: Option<PendingSegment>,
    established: bool,
    accepted: bool,
    received_fin: bool,
    ack_due: bool,
    received: VecDeque<u8>,
}

impl Peer {
    fn queue(&mut self, flags: u8, bytes: &[u8]) {
        let sequence = self.send_next;
        let control = u32::from(flags & (FLAG_SYN | FLAG_FIN) != 0);
        self.send_next = sequence.wrapping_add(bytes.len() as u32 + control);
        self.flight = Some(PendingSegment {
            sequence,
            flags: flags | FLAG_ACK,
            bytes: bytes.to_vec(),
            due: 0,
            attempts: 0,
        });
    }

    fn receive(&mut self, segment: TcpSegment<'_>) {
        let header = segment.header;
        if header.sequence != self.recv_next {
            self.ack_due = true;
            return;
        }
        if header.flags & FLAG_ACK == 0 {
            return;
        }
        // One outstanding segment. ACKs outside its range cannot advance the stream.
        if header.acknowledgment == self.send_next {
            self.flight = None;
            self.established = true;
        } else if let Some(pending) = self.flight.as_mut() {
            let advance = header.acknowledgment.wrapping_sub(pending.sequence) as usize;
            if !self.established || advance > pending.bytes.len() {
                return;
            }
            pending.bytes.drain(..advance);
            pending.sequence = header.acknowledgment;
        } else {
            return;
        }
        self.window = header.window as usize;
        if segment.payload.is_empty() && header.flags & FLAG_FIN == 0 {
            return;
        }
        self.ack_due = true;
        if self.received_fin {
            return;
        }
        let n = segment.payload.len().min(RX_CAPACITY - self.received.len());
        self.received.extend(&segment.payload[..n]);
        self.recv_next = self.recv_next.wrapping_add(n as u32);
        if n == segment.payload.len() && header.flags & FLAG_FIN != 0 {
            self.recv_next = self.recv_next.wrapping_add(1);
            self.received_fin = true;
        }
    }

    fn transmit(&mut self, device: &mut impl FrameDevice, now: u64) -> bool {
        let retry = self
            .flight
            .as_ref()
            .is_some_and(|pending| now >= pending.due);
        if !retry && !self.ack_due {
            return true;
        }
        let (sequence, flags, data) = if retry {
            let pending = self.flight.as_mut().unwrap();
            if pending.attempts == RETRY_LIMIT {
                return false;
            }
            pending.attempts += 1;
            pending.due = now + RETRY_MS;
            (pending.sequence, pending.flags, pending.bytes.as_slice())
        } else {
            (self.send_next, FLAG_ACK, &[][..])
        };
        let mut frame = [0; MAX_FRAME_LEN];
        let size = write_segment_with_window(
            &mut frame[TCP_OFFSET..],
            self.endpoints,
            sequence,
            self.recv_next,
            flags,
            data,
            (RX_CAPACITY - self.received.len()) as u16,
        )
        .unwrap();
        frame[..ETHERNET_HEADER_LEN].copy_from_slice(
            &EthernetHeader {
                dst: self.mac,
                src: device.mac_address(),
                ethertype: ETHERTYPE_IPV4,
            }
            .to_bytes(),
        );
        frame[ETHERNET_HEADER_LEN..TCP_OFFSET].copy_from_slice(&Ipv4Header::build(
            PROTOCOL_TCP,
            self.endpoints.local_ip,
            self.endpoints.remote_ip,
            size,
        ));
        self.ack_due = !device.transmit(&frame[..TCP_OFFSET + size]);
        true
    }
}

#[derive(Default)]
struct Sockets {
    allocated: bool,
    listening: bool,
    port: u16,
    peer: Option<Peer>,
}

static mut SOCKETS: Sockets = Sockets {
    allocated: false,
    listening: false,
    port: 0,
    peer: None,
};
// Each borrow ends before calling poll_ethernet_and_reply, which can re-enter
// receive/flush. Only the foreground program can own descriptors 3 and 4.
fn sockets<T>(operation: impl FnOnce(&mut Sockets) -> T) -> T {
    unsafe { operation(&mut *core::ptr::addr_of_mut!(SOCKETS)) }
}

pub fn reset() {
    sockets(|sockets_state| *sockets_state = Sockets::default());
}

pub fn receive(local_ip: [u8; 4], local_mac: [u8; 6], frame: &[u8]) {
    let Some(eth) = EthernetHeader::parse(frame) else {
        return;
    };
    if eth.ethertype != ETHERTYPE_IPV4 || eth.dst != local_mac || local_ip == [0; 4] {
        return;
    }
    let packet = &frame[ETHERNET_HEADER_LEN..];
    let Some(ip) = Ipv4Header::parse(packet) else {
        return;
    };
    if ip.protocol != PROTOCOL_TCP || ip.dst_ip != local_ip {
        return;
    }
    let Some(segment) = TcpSegment::parse(
        &packet[IPV4_HEADER_LEN..ip.total_len as usize],
        ip.src_ip,
        ip.dst_ip,
    ) else {
        return;
    };
    let header = segment.header;
    sockets(|sockets_state| {
        if let Some(peer) = sockets_state.peer.as_mut() {
            if peer.endpoints.local_port != header.dst_port
                || peer.endpoints.remote_port != header.src_port
                || peer.endpoints.remote_ip != ip.src_ip
            {
                return;
            }
            if header.flags & FLAG_RST != 0 {
                if header.sequence == peer.recv_next {
                    sockets_state.peer = None;
                }
            } else if header.flags & FLAG_SYN != 0 {
                if !peer.established && header.sequence.wrapping_add(1) == peer.recv_next {
                    if let Some(pending) = peer.flight.as_mut() {
                        pending.due = 0;
                    }
                }
            } else {
                peer.receive(segment);
            }
        } else if sockets_state.listening
            && header.dst_port == sockets_state.port
            && header.flags & (FLAG_SYN | FLAG_RST | FLAG_ACK | FLAG_FIN) == FLAG_SYN
        {
            let mut peer = Peer {
                endpoints: Endpoints {
                    local_ip,
                    remote_ip: ip.src_ip,
                    local_port: sockets_state.port,
                    remote_port: header.src_port,
                },
                mac: eth.src,
                send_next: (crate::raspi2b::timer::uptime_millis() as u32).wrapping_mul(1103515245),
                recv_next: header.sequence.wrapping_add(1),
                window: header.window as usize,
                flight: None,
                established: false,
                accepted: false,
                received_fin: false,
                ack_due: false,
                received: VecDeque::new(),
            };
            peer.queue(FLAG_SYN, &[]);
            sockets_state.peer = Some(peer);
        }
    });
}

pub fn flush(device: &mut impl FrameDevice, local_ip: [u8; 4]) {
    sockets(|sockets_state| {
        if let Some(peer) = sockets_state.peer.as_mut() {
            if peer.endpoints.local_ip != local_ip
                || !peer.transmit(device, crate::raspi2b::timer::uptime_millis())
            {
                sockets_state.peer = None;
            }
        }
    });
}

// No socket borrow or user-buffer reference crosses this call. SVC normally
// masks IRQs; allow the existing timer handler to run while waiting for Wi-Fi.
fn wait_for(mut ready: impl FnMut(&mut Sockets) -> Option<u32>) -> u32 {
    let mut console = crate::console::Pl011::new(crate::platform::UART_BASE);
    let mut unused_discovery_tick = 0;
    unsafe {
        core::arch::asm!("cpsie i", "isb");
    }
    loop {
        if let Some(value) = sockets(&mut ready) {
            return value;
        }
        crate::raspi2b::reset::arm_deadman(10);
        crate::flush_diagnostics(false);
        crate::poll_ethernet_and_reply(&mut console, &mut unused_discovery_tick);
        // Sleeps at most one 100 Hz timer period when the network is idle.
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

pub fn syscall(number: u32, fd: u32, arg: u32, len: u32) -> Option<u32> {
    if !super::wifi::running() {
        return None;
    }
    let result = match number {
        SYS_SOCKET => sockets(|sockets_state| {
            if !sockets_state.allocated
                && fd == AF_INET
                && arg == SOCK_STREAM
                && (len == 0 || len == u32::from(PROTOCOL_TCP))
            {
                sockets_state.allocated = true;
                LISTENER
            } else {
                ERROR
            }
        }),
        SYS_BIND if fd == LISTENER => {
            if len < 16 || !crate::memory::bootstrap_user_range(arg, 16) {
                return Some(ERROR);
            }
            let addr = unsafe { core::slice::from_raw_parts(arg as *const u8, 16) };
            let port = u16::from_be_bytes([addr[2], addr[3]]);
            sockets(|sockets_state| {
                if sockets_state.allocated
                    && sockets_state.port == 0
                    && u16::from_ne_bytes([addr[0], addr[1]]) == 2
                    && addr[4..8] == [0; 4]
                    && port != 0
                {
                    sockets_state.port = port;
                    0
                } else {
                    ERROR
                }
            })
        }
        SYS_LISTEN if fd == LISTENER => sockets(|sockets_state| {
            if sockets_state.port != 0 && sockets_state.allocated && arg > 0 {
                sockets_state.listening = true;
                0
            } else {
                ERROR
            }
        }),
        SYS_ACCEPT if fd == LISTENER && arg == 0 && len == 0 => wait_for(|sockets_state| {
            if !sockets_state.listening {
                return Some(ERROR);
            }
            let peer = sockets_state.peer.as_mut()?;
            if peer.established && !peer.accepted {
                peer.accepted = true;
                Some(STREAM)
            } else {
                None
            }
        }),
        SYS_READ | SYS_WRITE if fd == STREAM => {
            let size = (len as usize).min(if number == SYS_READ {
                RX_CAPACITY
            } else {
                MAX_SEND
            });
            if size != 0 && !crate::memory::bootstrap_user_range(arg, size as u32) {
                return Some(ERROR);
            }
            wait_for(|sockets_state| {
                let Some(peer) = sockets_state.peer.as_mut().filter(|peer| peer.accepted) else {
                    return Some(ERROR);
                };
                if size == 0 {
                    return Some(0);
                }
                if number == SYS_READ {
                    if peer.received.is_empty() {
                        return peer.received_fin.then_some(0);
                    }
                    let count = size.min(peer.received.len());
                    for (i, b) in peer.received.drain(..count).enumerate() {
                        unsafe {
                            (arg as *mut u8).add(i).write(b);
                        }
                    }
                    peer.ack_due = true;
                    Some(count as u32)
                } else {
                    if peer.flight.is_some() || peer.window == 0 {
                        return None;
                    }
                    let count = size.min(peer.window);
                    let data = unsafe { core::slice::from_raw_parts(arg as *const u8, count) };
                    peer.queue(FLAG_PSH, data);
                    Some(count as u32)
                }
            })
        }
        SYS_CLOSE if fd == STREAM => {
            let ready = wait_for(|sockets_state| match sockets_state.peer.as_mut() {
                Some(peer) if peer.accepted && peer.flight.is_none() => {
                    peer.queue(FLAG_FIN, &[]);
                    Some(0)
                }
                Some(peer) if peer.accepted => None,
                _ => Some(ERROR),
            });
            if ready == ERROR {
                ERROR
            } else {
                let closed = wait_for(|sockets_state| match &sockets_state.peer {
                    None => Some(ERROR),
                    Some(peer) => peer.flight.is_none().then_some(0),
                });
                sockets(|sockets_state| sockets_state.peer = None);
                closed
            }
        }
        SYS_CLOSE if fd == LISTENER => sockets(|sockets_state| {
            if sockets_state.allocated {
                sockets_state.allocated = false;
                sockets_state.listening = false;
                sockets_state.port = 0;
                0
            } else {
                ERROR
            }
        }),
        _ => return None,
    };
    // write() has accepted these bytes. Submit them even if the next user
    // instruction performs computation instead of immediately calling read().
    if number == SYS_WRITE && fd == STREAM && result != ERROR && result != 0 {
        crate::poll_ethernet_and_reply(
            &mut crate::console::Pl011::new(crate::platform::UART_BASE),
            &mut 0,
        );
    }
    Some(result)
}
