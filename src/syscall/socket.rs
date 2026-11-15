//! Socket and network-configuration syscall handlers.

use crate::net;
use crate::net::device as net_device;
use crate::println;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::user::is_low_half_range;

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

pub(super) static ACTIVE_TCP_FD: AtomicU64 = AtomicU64::new(u64::MAX);

pub(super) fn connect(fd: u64, address: u64, address_len: u64) -> u64 {
    if crate::task::with_current_fd_table(|table| table.get_tcp_socket(fd)).is_none() {
        return u64::MAX;
    }
    let Some(remote) = read_sockaddr(address, address_len) else {
        return u64::MAX;
    };
    let local_ip = net::source_ip();
    if local_ip == [0; 4] {
        return u64::MAX;
    }
    if ACTIVE_TCP_FD
        .compare_exchange(u64::MAX, fd, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return u64::MAX;
    }
    let isn = (crate::arch::timer_ticks() as u32)
        .wrapping_mul(1103515245)
        .wrapping_add(0x31c0_ffee);
    let Some(syn) = net::tcp::active_open(
        net::tcp::Endpoints {
            local_ip,
            remote_ip: remote.ip,
            local_port: 49152,
            remote_port: remote.port,
        },
        isn,
    ) else {
        ACTIVE_TCP_FD.store(u64::MAX, Ordering::Release);
        return u64::MAX;
    };
    if !transmit_tcp(syn) {
        ACTIVE_TCP_FD.store(u64::MAX, Ordering::Release);
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

fn read_sockaddr(ptr: u64, len: u64) -> Option<net::socket::Endpoint> {
    if len < 16 || !is_low_half_range(ptr, 16) {
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, 16) };
    if u16::from_ne_bytes([bytes[0], bytes[1]]) != AF_INET as u16 {
        return None;
    }
    Some(net::socket::Endpoint {
        port: u16::from_be_bytes([bytes[2], bytes[3]]),
        ip: [bytes[4], bytes[5], bytes[6], bytes[7]],
    })
}

fn write_sockaddr(ptr: u64, len_ptr: u64, endpoint: net::socket::Endpoint) -> bool {
    if ptr == 0 && len_ptr == 0 {
        return true;
    }
    if !is_low_half_range(len_ptr, 4) {
        return false;
    }
    let capacity = unsafe { *(len_ptr as *const u32) } as u64;
    if capacity < 16 || !is_low_half_range(ptr, 16) {
        return false;
    }
    let out = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, 16) };
    out.fill(0);
    out[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    out[2..4].copy_from_slice(&endpoint.port.to_be_bytes());
    out[4..8].copy_from_slice(&endpoint.ip);
    unsafe {
        *(len_ptr as *mut u32) = 16;
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
        SOCK_DGRAM if protocol == 0 || protocol == 17 => {
            let socket = net::socket::UdpSocket::new(kind & SOCK_NONBLOCK != 0);
            crate::task::with_current_fd_table(|fdt| fdt.open_socket(socket))
        }
        SOCK_STREAM if protocol == 0 || protocol == 6 => {
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
    if endpoint.ip != [0, 0, 0, 0] && endpoint.ip != net::source_ip() {
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
    if socket.listen(backlog.min(128) as usize).is_ok() {
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
    if dst.ip != [255; 4] && net_device::resolve_next_hop(dst.ip).is_none() {
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
        0 => match net_device::with_net_device(|device| device.mac()) {
            Some(mac) => {
                config.mac = mac;
                0
            }
            None => u64::MAX,
        },
        1 if config.address != [0; 4] && config.subnet_mask != [0; 4] => {
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

/// Lesson 27 poll subset: UDP descriptors, POLLIN, and timeout 0 or -1.
pub(super) fn poll(fds_ptr: u64, nfds: u64, timeout: u64) -> u64 {
    const MAX_POLL_FDS: u64 = 64;
    if nfds > MAX_POLL_FDS || (timeout != 0 && timeout != u64::MAX) {
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
        let should_block = crate::arch::interrupts::without_interrupts(|| {
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
            if ready != 0 || timeout == 0 {
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
        if let Some(ready) = should_block {
            return ready;
        }
        crate::task::yield_now();
    }
}
