//! Socket and network-configuration syscall handlers.
//!
//! User pointers receive numeric-bound checks only; callers must currently
//! supply mapped, accessible storage with the alignment required by each type.

use crate::net;
use crate::net::device as net_device;
use crate::println;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::user::is_low_half_range;

// Linux sockaddr_in byte layout. Family uses native byte order; port and
// IPv4 address use network byte order. The final eight bytes are padding.
mod sockaddr_in {
    pub const BYTE_LEN: usize = 16;
    pub const FAMILY: core::ops::Range<usize> = 0..2;
    pub const PORT: core::ops::Range<usize> = 2..4;
    pub const ADDRESS: core::ops::Range<usize> = 4..8;
}
const SOCKET_LENGTH_BYTES: u64 = core::mem::size_of::<u32>() as u64;
const DEFAULT_PROTOCOL: u64 = 0;
const IPV4_UNSPECIFIED: [u8; 4] = [0; 4];
const IPV4_LIMITED_BROADCAST: [u8; 4] = [255; 4];
const NO_ACTIVE_TCP_FD: u64 = u64::MAX;
const ACTIVE_TCP_LOCAL_PORT: u16 = 49152;
// Existing deterministic initial sequence-number calculation. Timer ticks
// are predictable; this provides no cryptographic randomness.
const INITIAL_SEQUENCE_MULTIPLIER: u32 = 1103515245;
const INITIAL_SEQUENCE_OFFSET: u32 = 0x31c0_ffee;
const MAX_LISTEN_BACKLOG: u64 = 128;
const POLL_IMMEDIATE: u64 = 0;
const POLL_WAIT_FOREVER: u64 = (-1i64) as u64;
const NET_CONFIG_READ_MAC: u64 = 0;
const NET_CONFIG_INSTALL_LEASE: u64 = 1;

pub(super) fn transmit_tcp(outbound: net::tcp::Outbound) -> bool {
    if net_device::resolve_next_hop(outbound.dst_ip).is_none() {
        return false;
    }
    matches!(
        net_device::with_net_device(|device| device.send_ipv4_packet(
            net::ipv4::PROTOCOL_TCP,
            outbound.dst_ip,
            &outbound.bytes
        )),
        Some(Some(()))
    )
}

// A single global active-open connection, identified only by descriptor number.
// The current design does not distinguish equal FD numbers in different tasks.
pub(super) static ACTIVE_TCP_FD: AtomicU64 = AtomicU64::new(NO_ACTIVE_TCP_FD);

pub(super) fn connect(fd: u64, address: u64, address_len: u64) -> u64 {
    if crate::task::with_current_fd_table(|table| table.get_tcp_socket(fd)).is_none() {
        return u64::MAX;
    }
    let Some(remote) = read_sockaddr(address, address_len) else {
        return u64::MAX;
    };
    let local_ip = net::source_ip();
    if local_ip == IPV4_UNSPECIFIED {
        return u64::MAX;
    }
    if ACTIVE_TCP_FD
        .compare_exchange(NO_ACTIVE_TCP_FD, fd, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return u64::MAX;
    }
    let isn = (crate::arch::timer_ticks() as u32)
        .wrapping_mul(INITIAL_SEQUENCE_MULTIPLIER)
        .wrapping_add(INITIAL_SEQUENCE_OFFSET);
    let Some(syn) = net::tcp::active_open(
        net::tcp::Endpoints {
            local_ip,
            remote_ip: remote.ip,
            local_port: ACTIVE_TCP_LOCAL_PORT,
            remote_port: remote.port,
        },
        isn,
    ) else {
        ACTIVE_TCP_FD.store(NO_ACTIVE_TCP_FD, Ordering::Release);
        return u64::MAX;
    };
    if !transmit_tcp(syn) {
        ACTIVE_TCP_FD.store(NO_ACTIVE_TCP_FD, Ordering::Release);
        return u64::MAX;
    }
    while net::tcp::state() != net::tcp::TcpState::Established {
        crate::task::block_current_if(|pid| net::tcp::register_connect_waiter(pid));
        crate::task::yield_now();
    }
    0
}

pub(super) fn active_tcp_send(payload: &[u8]) -> Option<usize> {
    let Some((accepted, segments)) = net::tcp::send_data(payload) else {
        return None;
    };
    for segment in segments {
        if !transmit_tcp(segment) {
            return None;
        }
    }
    Some(accepted)
}

pub(super) fn active_tcp_recv(output: &mut [u8]) -> usize {
    loop {
        let mut count = 0;
        while count < output.len() {
            let Some(byte) = net::tcp::pop_byte() else {
                break;
            };
            output[count] = byte;
            count += 1;
        }
        if count != 0 {
            return count;
        }
        crate::task::block_current_if(net::tcp::register_recv_waiter);
        crate::task::yield_now();
    }
}

const AF_INET: u64 = 2;
const SOCK_STREAM: u64 = 1;
const SOCK_DGRAM: u64 = 2;
const SOCK_NONBLOCK: u64 = 0x800;
const EAGAIN: u64 = (-11i64) as u64;
const POLLIN: i16 = 0x0001;
const POLLNVAL: i16 = 0x0020;

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

// The numeric range check does not verify that these bytes are mapped and
// readable. The ABI decoder currently relies on that caller requirement.
fn read_sockaddr(ptr: u64, len: u64) -> Option<net::socket::Endpoint> {
    if len < sockaddr_in::BYTE_LEN as u64 || !is_low_half_range(ptr, sockaddr_in::BYTE_LEN as u64) {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, sockaddr_in::BYTE_LEN) };
    if u16::from_ne_bytes(bytes[sockaddr_in::FAMILY].try_into().unwrap()) != AF_INET as u16 {
        return None;
    }
    Some(net::socket::Endpoint {
        port: u16::from_be_bytes(bytes[sockaddr_in::PORT].try_into().unwrap()),
        ip: bytes[sockaddr_in::ADDRESS].try_into().unwrap(),
    })
}

// Two null pointers request no sender address. Otherwise len_ptr must refer
// to an aligned, writable u32 and ptr to a writable full sockaddr_in. Short
// buffers are rejected rather than receiving a truncated address.
fn write_sockaddr(ptr: u64, len_ptr: u64, endpoint: net::socket::Endpoint) -> bool {
    if ptr == 0 && len_ptr == 0 {
        return true;
    }
    if !is_low_half_range(len_ptr, SOCKET_LENGTH_BYTES) {
        return false;
    }
    let capacity = unsafe { *(len_ptr as *const u32) } as u64;
    if capacity < sockaddr_in::BYTE_LEN as u64
        || !is_low_half_range(ptr, sockaddr_in::BYTE_LEN as u64)
    {
        return false;
    }
    let out = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, sockaddr_in::BYTE_LEN) };
    out.fill(0);
    out[sockaddr_in::FAMILY].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    out[sockaddr_in::PORT].copy_from_slice(&endpoint.port.to_be_bytes());
    out[sockaddr_in::ADDRESS].copy_from_slice(&endpoint.ip);
    unsafe {
        *(len_ptr as *mut u32) = sockaddr_in::BYTE_LEN as u32;
    }
    true
}

pub(super) fn socket(domain: u64, kind: u64, protocol: u64) -> u64 {
    net_device::ensure_network_task();
    let base_kind = kind & !SOCK_NONBLOCK;
    if domain != AF_INET {
        return u64::MAX;
    }
    match base_kind {
        SOCK_DGRAM
            if protocol == DEFAULT_PROTOCOL || protocol == net::ipv4::PROTOCOL_UDP as u64 =>
        {
            let socket = net::socket::UdpSocket::new(kind & SOCK_NONBLOCK != 0);
            crate::task::with_current_fd_table(|fdt| fdt.open_socket(socket))
        }
        SOCK_STREAM
            if protocol == DEFAULT_PROTOCOL || protocol == net::ipv4::PROTOCOL_TCP as u64 =>
        {
            let socket = net::tcp::TcpSocket::new();
            crate::task::with_current_fd_table(|fdt| fdt.open_tcp_socket(socket))
        }
        _ => u64::MAX,
    }
}

pub(super) fn bind(fd: u64, addr: u64, addr_len: u64) -> u64 {
    let Some(endpoint) = read_sockaddr(addr, addr_len) else {
        return u64::MAX;
    };
    if endpoint.ip != IPV4_UNSPECIFIED && endpoint.ip != net::source_ip() {
        return u64::MAX;
    }
    let bound = crate::task::with_current_fd_table(|fdt| {
        if let Some(socket) = fdt.get_socket(fd) {
            return socket.bind(endpoint.port).is_ok();
        }
        if let Some(socket) = fdt.get_tcp_socket(fd) {
            return socket.bind(endpoint.port).is_ok();
        }
        false
    });
    if bound {
        0
    } else {
        u64::MAX
    }
}

pub(super) fn listen(fd: u64, backlog: u64) -> u64 {
    let Some(socket) = crate::task::with_current_fd_table(|fdt| fdt.get_tcp_socket(fd)) else {
        return u64::MAX;
    };
    if socket
        .listen(backlog.min(MAX_LISTEN_BACKLOG) as usize)
        .is_ok()
    {
        0
    } else {
        u64::MAX
    }
}

pub(super) fn accept(fd: u64) -> u64 {
    let Some(listener) = crate::task::with_current_fd_table(|fdt| fdt.get_tcp_socket(fd)) else {
        return u64::MAX;
    };
    loop {
        if let Some(connection) = listener.pop_accepted() {
            let accepted_fd =
                crate::task::with_current_fd_table(|fdt| fdt.open_tcp_socket(connection));
            println!("[tcp] accept fd={} -> connection fd={}", fd, accepted_fd);
            return accepted_fd;
        }
        crate::task::block_current_if(|pid| listener.register_accept_waiter(pid));
        crate::task::yield_now();
    }
}

pub(super) fn sendto(fd: u64, buf: u64, len: u64, _flags: u64, addr: u64, addr_len: u64) -> u64 {
    if !is_low_half_range(buf, len) {
        return u64::MAX;
    }
    let Some(dst) = read_sockaddr(addr, addr_len) else {
        return u64::MAX;
    };
    let Some(socket) = crate::task::with_current_fd_table(|fdt| fdt.get_socket(fd)) else {
        return u64::MAX;
    };
    let Some(src_port) = socket.local_port() else {
        return u64::MAX;
    };
    let payload = unsafe { core::slice::from_raw_parts(buf as *const u8, len as usize) };
    // Resolve the next hop BEFORE taking the device lock. A cold ARP
    // cache means sleeping until the reply arrives, and the network task
    // needs that same lock to deliver the reply.
    if dst.ip != IPV4_LIMITED_BROADCAST && net_device::resolve_next_hop(dst.ip).is_none() {
        return u64::MAX;
    }
    let result = match net_device::with_net_device(|device| {
        device.send_udp(dst.ip, dst.port, src_port, payload)
    }) {
        Some(Some(())) => len,
        _ => u64::MAX,
    };
    result
}

#[repr(C)]
struct UserNetworkConfig {
    address: [u8; 4],
    subnet_mask: [u8; 4],
    gateway: [u8; 4],
    dns: [u8; 4],
    lease_seconds: u32,
    mac: [u8; 6],
    _padding: [u8; 2],
}

/// Operation 0 copies the device MAC address to user memory. Operation 1
/// installs the lease fields supplied by the DHCP client.
pub(super) fn net_configure(config_ptr: u64, operation: u64) -> u64 {
    if !is_low_half_range(config_ptr, core::mem::size_of::<UserNetworkConfig>() as u64) {
        return u64::MAX;
    }
    let config = unsafe { &mut *(config_ptr as *mut UserNetworkConfig) };
    match operation {
        NET_CONFIG_READ_MAC => match net_device::with_net_device(|device| device.mac()) {
            Some(mac) => {
                config.mac = mac;
                0
            }
            None => u64::MAX,
        },
        NET_CONFIG_INSTALL_LEASE
            if config.address != IPV4_UNSPECIFIED && config.subnet_mask != [0; 4] =>
        {
            net::configure(net::NetworkConfig {
                address: config.address,
                subnet_mask: config.subnet_mask,
                gateway: config.gateway,
                dns: config.dns,
                lease_seconds: config.lease_seconds,
            });
            println!("[dhcp] configured address={}.{}.{}.{} mask={}.{}.{}.{} gateway={}.{}.{}.{} dns={}.{}.{}.{} lease={}s",
                config.address[0], config.address[1], config.address[2], config.address[3],
                config.subnet_mask[0], config.subnet_mask[1], config.subnet_mask[2], config.subnet_mask[3],
                config.gateway[0], config.gateway[1], config.gateway[2], config.gateway[3],
                config.dns[0], config.dns[1], config.dns[2], config.dns[3], config.lease_seconds);
            0
        }
        _ => u64::MAX,
    }
}

/// Pop one UDP datagram. A short output buffer discards the remaining bytes.
/// Sender-address validation happens after dequeue and payload copying, so an
/// invalid address result can consume a datagram before returning an error.
/// The flags argument is currently ignored.
pub(super) fn recvfrom(fd: u64, buf: u64, len: u64, _flags: u64, addr: u64, addr_len: u64) -> u64 {
    if !is_low_half_range(buf, len) {
        return u64::MAX;
    }
    let Some(socket) = crate::task::with_current_fd_table(|fdt| fdt.get_socket(fd)) else {
        return u64::MAX;
    };
    loop {
        if let Some(datagram) = socket.pop() {
            let count = core::cmp::min(len as usize, datagram.payload.len());
            unsafe {
                core::ptr::copy_nonoverlapping(datagram.payload.as_ptr(), buf as *mut u8, count);
            }
            if !write_sockaddr(addr, addr_len, datagram.sender) {
                return u64::MAX;
            }
            return count as u64;
        }
        if socket.is_nonblocking() {
            return EAGAIN;
        }
        // Keep interrupts disabled from waiter publication through the
        // context switch. Re-enabling between "Blocked" and `yield_now`
        // lets a timer preempt this syscall on its entry stack before the
        // deliberate switch has saved that context.
        crate::arch::interrupts::disable();
        let should_block =
            crate::task::block_current_if(|pid| socket.register_waiter_if_empty(pid));
        if should_block {
            crate::task::yield_now();
        } else {
            crate::arch::interrupts::enable();
        }
    }
}

/// Poll subset: UDP descriptors, POLLIN, and timeout 0 or -1. No timed wait.
/// Negative descriptors are ignored; unsupported/nonexistent descriptors report
/// POLLNVAL. User storage must remain valid across any scheduler wait.
pub(super) fn poll(fds_ptr: u64, nfds: u64, timeout: u64) -> u64 {
    const MAX_POLL_FDS: u64 = 64;
    if nfds > MAX_POLL_FDS || (timeout != POLL_IMMEDIATE && timeout != POLL_WAIT_FOREVER) {
        return u64::MAX;
    }
    let bytes = match nfds.checked_mul(core::mem::size_of::<PollFd>() as u64) {
        Some(bytes) => bytes,
        None => return u64::MAX,
    };
    if !is_low_half_range(fds_ptr, bytes) {
        return u64::MAX;
    }

    let pollfds = unsafe { core::slice::from_raw_parts_mut(fds_ptr as *mut PollFd, nfds as usize) };
    let sockets: Vec<Option<Arc<net::socket::UdpSocket>>> = pollfds
        .iter()
        .map(|entry| {
            if entry.fd < 0 {
                None
            } else {
                crate::task::with_current_fd_table(|table| table.get_socket(entry.fd as u64))
            }
        })
        .collect();
    let pid = crate::task::current_pid();

    loop {
        let ready_count = crate::arch::interrupts::without_interrupts(|| {
            for socket in sockets.iter().flatten() {
                socket.remove_poll_waiter(pid);
            }

            let mut ready = 0u64;
            for (entry, socket) in pollfds.iter_mut().zip(sockets.iter()) {
                entry.revents = 0;
                if entry.fd < 0 {
                    continue;
                }
                match socket {
                    None => entry.revents = POLLNVAL,
                    Some(socket) if entry.events & POLLIN != 0 && socket.is_readable() => {
                        entry.revents = POLLIN;
                    }
                    Some(_) => {}
                }
                if entry.revents != 0 {
                    ready += 1;
                }
            }
            if ready != 0 || timeout == POLL_IMMEDIATE {
                return Some(ready);
            }

            for (entry, socket) in pollfds.iter().zip(sockets.iter()) {
                if entry.events & POLLIN != 0 {
                    if let Some(socket) = socket {
                        socket.register_poll_waiter_if_empty(pid);
                    }
                }
            }
            crate::task::block_current();
            None
        });
        if let Some(ready) = ready_count {
            return ready;
        }
        crate::task::yield_now();
    }
}
