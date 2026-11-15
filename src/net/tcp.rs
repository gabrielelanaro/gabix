//! Minimal active-open TCP with a reliable send stream.

use crate::irq_mutex::IrqMutex;
use crate::net::checksum;
use crate::println;
use crate::process::Pid;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;

pub const PROTOCOL_TCP: u8 = 6;
pub const TCP_HEADER_LEN: usize = 20;
pub const FLAG_FIN: u8 = 0x01;
pub const FLAG_SYN: u8 = 0x02;
pub const FLAG_RST: u8 = 0x04;
pub const FLAG_PSH: u8 = 0x08;
pub const FLAG_ACK: u8 = 0x10;
const CLOCK_GRANULARITY_TICKS: usize = 1;
const INITIAL_RTO_TICKS: usize = 100;
const MAX_RTO_TICKS: usize = 6000;
const SEND_MSS: usize = 1460;
const CONGESTION_WINDOW_SEGMENTS: usize = 4;

// RFC 9293 section 3.1: byte ranges in the fixed TCP header.
mod header_field {
    pub const SOURCE_PORT: core::ops::Range<usize> = 0..2;
    pub const DESTINATION_PORT: core::ops::Range<usize> = 2..4;
    pub const SEQUENCE: core::ops::Range<usize> = 4..8;
    pub const ACKNOWLEDGMENT: core::ops::Range<usize> = 8..12;
    pub const DATA_OFFSET: usize = 12;
    pub const FLAGS: usize = 13;
    pub const WINDOW: core::ops::Range<usize> = 14..16;
    pub const CHECKSUM: core::ops::Range<usize> = 16..18;
}
const HEADER_WORD_BYTES: usize = 4;
const DATA_OFFSET_SHIFT: u32 = 4;
// Advertised values are fixed policy here, independent of receive-queue usage.
const ADVERTISED_RECEIVE_WINDOW: u16 = 4096;
const ADVERTISED_MSS: u16 = 1460;
const MSS_OPTION_KIND: u8 = 2;
const MSS_OPTION_BYTES: usize = 4;
const SYN_HEADER_BYTES: usize = TCP_HEADER_LEN + MSS_OPTION_BYTES;

// IPv4 pseudo-header used only for checksum calculation, not transmitted here.
mod pseudo_header {
    pub const BYTE_LEN: usize = 12;
    pub const SOURCE_IP: core::ops::Range<usize> = 0..4;
    pub const DESTINATION_IP: core::ops::Range<usize> = 4..8;
    pub const PROTOCOL: usize = 9;
    pub const TCP_LENGTH: core::ops::Range<usize> = 10..12;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub sequence: u32,
    pub acknowledgment: u32,
    pub flags: u8,
    pub window: u16,
    pub header_len: usize,
}

pub struct TcpSegment<'a> {
    pub header: TcpHeader,
    pub payload: &'a [u8],
}

impl<'a> TcpSegment<'a> {
    /// Validate minimum/header lengths and the checksum over all supplied
    /// bytes. Options are skipped using Data Offset; their contents and lengths
    /// are not interpreted or negotiated here.
    pub fn parse(bytes: &'a [u8], src_ip: [u8; 4], dst_ip: [u8; 4]) -> Option<Self> {
        if bytes.len() < TCP_HEADER_LEN || tcp_checksum(src_ip, dst_ip, bytes) != 0 {
            return None;
        }
        let header_len =
            ((bytes[header_field::DATA_OFFSET] >> DATA_OFFSET_SHIFT) as usize) * HEADER_WORD_BYTES;
        if header_len < TCP_HEADER_LEN || header_len > bytes.len() {
            return None;
        }
        Some(Self {
            header: TcpHeader {
                src_port: u16::from_be_bytes(bytes[header_field::SOURCE_PORT].try_into().ok()?),
                dst_port: u16::from_be_bytes(
                    bytes[header_field::DESTINATION_PORT].try_into().ok()?,
                ),
                sequence: u32::from_be_bytes(bytes[header_field::SEQUENCE].try_into().ok()?),
                acknowledgment: u32::from_be_bytes(
                    bytes[header_field::ACKNOWLEDGMENT].try_into().ok()?,
                ),
                flags: bytes[header_field::FLAGS],
                window: u16::from_be_bytes(bytes[header_field::WINDOW].try_into().ok()?),
                header_len,
            },
            payload: &bytes[header_len..],
        })
    }
}

/// Write a TCP segment. SYN segments include the four-byte MSS option.
pub fn write_segment(
    out: &mut [u8],
    endpoints: Endpoints,
    sequence: u32,
    acknowledgment: u32,
    flags: u8,
    payload: &[u8],
) -> Option<usize> {
    let header_len = if flags & FLAG_SYN != 0 {
        SYN_HEADER_BYTES
    } else {
        TCP_HEADER_LEN
    };
    let length = header_len.checked_add(payload.len())?;
    if length > u16::MAX as usize || out.len() < length {
        return None;
    }
    out[..length].fill(0);
    out[header_field::SOURCE_PORT].copy_from_slice(&endpoints.local_port.to_be_bytes());
    out[header_field::DESTINATION_PORT].copy_from_slice(&endpoints.remote_port.to_be_bytes());
    out[header_field::SEQUENCE].copy_from_slice(&sequence.to_be_bytes());
    out[header_field::ACKNOWLEDGMENT].copy_from_slice(&acknowledgment.to_be_bytes());
    out[header_field::DATA_OFFSET] = ((header_len / HEADER_WORD_BYTES) as u8) << DATA_OFFSET_SHIFT;
    out[header_field::FLAGS] = flags;
    out[header_field::WINDOW].copy_from_slice(&ADVERTISED_RECEIVE_WINDOW.to_be_bytes());
    if header_len == SYN_HEADER_BYTES {
        let mss_bytes = ADVERTISED_MSS.to_be_bytes();
        out[TCP_HEADER_LEN..SYN_HEADER_BYTES].copy_from_slice(&[
            MSS_OPTION_KIND,
            MSS_OPTION_BYTES as u8,
            mss_bytes[0],
            mss_bytes[1],
        ]);
    }
    out[header_len..length].copy_from_slice(payload);
    let sum = tcp_checksum(endpoints.local_ip, endpoints.remote_ip, &out[..length]);
    out[header_field::CHECKSUM].copy_from_slice(&sum.to_be_bytes());
    Some(length)
}

fn tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], bytes: &[u8]) -> u16 {
    let mut pseudo = [0u8; pseudo_header::BYTE_LEN];
    pseudo[pseudo_header::SOURCE_IP].copy_from_slice(&src_ip);
    pseudo[pseudo_header::DESTINATION_IP].copy_from_slice(&dst_ip);
    pseudo[pseudo_header::PROTOCOL] = PROTOCOL_TCP;
    pseudo[pseudo_header::TCP_LENGTH].copy_from_slice(&(bytes.len() as u16).to_be_bytes());
    checksum::internet_checksum_parts(&[&pseudo, bytes])
}

#[derive(Clone, Copy)]
pub struct Endpoints {
    pub local_ip: [u8; 4],
    pub remote_ip: [u8; 4],
    pub local_port: u16,
    pub remote_port: u16,
}

struct TcpControlBlock {
    endpoints: Endpoints,
    state: TcpState,
    send_next: u32,
    send_unacked: u32,
    recv_next: u32,
    send_window: u16,
    receive: Vec<u8>,
    connect_waiter: Option<Pid>,
    recv_waiter: Option<Pid>,
    retransmit_queue: Vec<RetransmitEntry>,
    rtt: RttEstimator,
}

static TCB: IrqMutex<Option<TcpControlBlock>> = IrqMutex::new(None);

pub struct Outbound {
    pub dst_ip: [u8; 4],
    pub bytes: Vec<u8>,
    pub sequence: u32,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ConnectionKey {
    local_ip: [u8; 4],
    remote_ip: [u8; 4],
    local_port: u16,
    remote_port: u16,
}

struct ServerConnection {
    endpoints: Endpoints,
    state: TcpState,
    send_next: u32,
    send_unacked: u32,
    recv_next: u32,
    receive: VecDeque<u8>,
    eof: bool,
    recv_waiter: Option<Pid>,
    close_waiter: Option<Pid>,
    listener: Weak<TcpSocket>,
}

enum SocketState {
    Unbound {
        port: Option<u16>,
    },
    Listener {
        port: u16,
        backlog: usize,
        accepted: VecDeque<Arc<TcpSocket>>,
        accept_waiter: Option<Pid>,
    },
    Connection(ServerConnection),
    Closed,
}

pub struct TcpSocket {
    inner: IrqMutex<SocketState>,
}

lazy_static::lazy_static! {
    static ref LISTENERS: IrqMutex<BTreeMap<u16, Weak<TcpSocket>>> =
        IrqMutex::new(BTreeMap::new());
    static ref CONNECTIONS: IrqMutex<BTreeMap<ConnectionKey, Arc<TcpSocket>>> =
        IrqMutex::new(BTreeMap::new());
}

impl TcpSocket {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: IrqMutex::new(SocketState::Unbound { port: None }),
        })
    }

    pub fn bind(&self, port: u16) -> Result<(), ()> {
        if port == 0
            || LISTENERS
                .lock()
                .get(&port)
                .and_then(Weak::upgrade)
                .is_some()
        {
            return Err(());
        }
        let mut state = self.inner.lock();
        match &mut *state {
            SocketState::Unbound { port: bound } if bound.is_none() => {
                *bound = Some(port);
                Ok(())
            }
            _ => Err(()),
        }
    }

    pub fn listen(self: &Arc<Self>, backlog: usize) -> Result<(), ()> {
        let port = match *self.inner.lock() {
            SocketState::Unbound { port: Some(port) } => port,
            _ => return Err(()),
        };
        let mut listeners = LISTENERS.lock();
        if listeners.get(&port).and_then(Weak::upgrade).is_some() {
            return Err(());
        }
        *self.inner.lock() = SocketState::Listener {
            port,
            backlog: backlog.max(1),
            accepted: VecDeque::new(),
            accept_waiter: None,
        };
        listeners.insert(port, Arc::downgrade(self));
        println!(
            "[tcp] state CLOSED -> LISTEN port={} backlog={}",
            port,
            backlog.max(1)
        );
        Ok(())
    }

    pub fn pop_accepted(&self) -> Option<Arc<TcpSocket>> {
        match &mut *self.inner.lock() {
            SocketState::Listener { accepted, .. } => accepted.pop_front(),
            _ => None,
        }
    }

    pub fn register_accept_waiter(&self, pid: Pid) -> bool {
        let mut state = self.inner.lock();
        match &mut *state {
            SocketState::Listener {
                accepted,
                accept_waiter,
                ..
            } if accepted.is_empty() => {
                *accept_waiter = Some(pid);
                true
            }
            _ => false,
        }
    }

    pub fn read(&self, out: &mut [u8]) -> Option<usize> {
        let mut state = self.inner.lock();
        let SocketState::Connection(connection) = &mut *state else {
            return None;
        };
        if connection.receive.is_empty() {
            return if connection.eof { Some(0) } else { None };
        }
        let count = out.len().min(connection.receive.len());
        for byte in &mut out[..count] {
            *byte = connection.receive.pop_front().unwrap();
        }
        Some(count)
    }

    pub fn register_recv_waiter(&self, pid: Pid) -> bool {
        let mut state = self.inner.lock();
        match &mut *state {
            SocketState::Connection(connection)
                if connection.receive.is_empty() && !connection.eof =>
            {
                connection.recv_waiter = Some(pid);
                true
            }
            _ => false,
        }
    }

    pub fn send(&self, payload: &[u8]) -> Option<Vec<Outbound>> {
        let mut state = self.inner.lock();
        let SocketState::Connection(connection) = &mut *state else {
            return None;
        };
        if connection.state != TcpState::Established && connection.state != TcpState::CloseWait {
            return None;
        }
        let mut result = Vec::new();
        for chunk in payload.chunks(1460) {
            let sequence = connection.send_next;
            result.push(build_server(
                connection,
                sequence,
                FLAG_PSH | FLAG_ACK,
                chunk,
            )?);
            connection.send_next = connection.send_next.wrapping_add(chunk.len() as u32);
        }
        Some(result)
    }

    pub fn close_segment(&self) -> Option<Outbound> {
        let mut state = self.inner.lock();
        let SocketState::Connection(connection) = &mut *state else {
            return None;
        };
        let next = match connection.state {
            TcpState::Established => TcpState::FinWait1,
            TcpState::CloseWait => TcpState::LastAck,
            _ => return None,
        };
        let outbound = build_server(connection, connection.send_next, FLAG_FIN | FLAG_ACK, &[])?;
        connection.send_next = connection.send_next.wrapping_add(1);
        println!(
            "[tcp] state {:?} -> {:?}; FIN seq={}",
            connection.state, next, outbound.sequence
        );
        connection.state = next;
        Some(outbound)
    }

    pub fn is_closed(&self) -> bool {
        matches!(
            &*self.inner.lock(),
            SocketState::Connection(ServerConnection {
                state: TcpState::Closed,
                ..
            })
        )
    }

    pub fn register_close_waiter(&self, pid: Pid) -> bool {
        let mut state = self.inner.lock();
        match &mut *state {
            SocketState::Connection(connection) if connection.state != TcpState::Closed => {
                connection.close_waiter = Some(pid);
                true
            }
            _ => false,
        }
    }
}

fn build_server(
    connection: &ServerConnection,
    sequence: u32,
    flags: u8,
    payload: &[u8],
) -> Option<Outbound> {
    let mut bytes = Vec::new();
    bytes.resize(24 + payload.len(), 0);
    let len = write_segment(
        &mut bytes,
        connection.endpoints,
        sequence,
        if flags & FLAG_ACK != 0 {
            connection.recv_next
        } else {
            0
        },
        flags,
        payload,
    )?;
    bytes.truncate(len);
    Some(Outbound {
        dst_ip: connection.endpoints.remote_ip,
        bytes,
        sequence,
        flags,
    })
}

#[derive(Clone)]
struct RetransmitEntry {
    sequence: u32,
    flags: u8,
    payload: Vec<u8>,
    sent_at: usize,
    retransmitted: bool,
}

impl RetransmitEntry {
    fn sequence_end(&self) -> u32 {
        self.sequence
            .wrapping_add(self.payload.len() as u32)
            .wrapping_add(((self.flags & (FLAG_SYN | FLAG_FIN)) != 0) as u32)
    }
}

struct RttEstimator {
    srtt_ticks: Option<usize>,
    rttvar_ticks: usize,
    computed_rto_ticks: usize,
    current_rto_ticks: usize,
}

impl RttEstimator {
    const fn new() -> Self {
        Self {
            srtt_ticks: None,
            rttvar_ticks: 0,
            computed_rto_ticks: INITIAL_RTO_TICKS,
            current_rto_ticks: INITIAL_RTO_TICKS,
        }
    }

    fn record_sample(&mut self, sample_ticks: usize) {
        let sample = sample_ticks.max(1);
        match self.srtt_ticks {
            None => {
                self.srtt_ticks = Some(sample);
                self.rttvar_ticks = (sample / 2).max(1);
            }
            Some(old_srtt) => {
                let deviation = old_srtt.abs_diff(sample);
                self.rttvar_ticks = (3 * self.rttvar_ticks + deviation) / 4;
                self.srtt_ticks = Some((7 * old_srtt + sample) / 8);
            }
        }
        let variation = (4 * self.rttvar_ticks).max(CLOCK_GRANULARITY_TICKS);
        self.computed_rto_ticks = self
            .srtt_ticks
            .unwrap_or(sample)
            .saturating_add(variation)
            .clamp(INITIAL_RTO_TICKS, MAX_RTO_TICKS);
        self.current_rto_ticks = self.computed_rto_ticks;
        println!(
            "[tcp] RTT sample={} ticks srtt={} rttvar={} rto={} ticks",
            sample,
            self.srtt_ticks.unwrap_or(sample),
            self.rttvar_ticks,
            self.current_rto_ticks
        );
    }

    fn back_off(&mut self) {
        self.current_rto_ticks = self.current_rto_ticks.saturating_mul(2).min(MAX_RTO_TICKS);
    }
}

fn now_ticks() -> usize {
    crate::arch::timer_ticks()
}

fn build_at(tcb: &TcpControlBlock, sequence: u32, flags: u8, payload: &[u8]) -> Option<Outbound> {
    let mut bytes = Vec::new();
    bytes.resize(24 + payload.len(), 0);
    let len = write_segment(
        &mut bytes,
        tcb.endpoints,
        sequence,
        if flags & FLAG_ACK != 0 {
            tcb.recv_next
        } else {
            0
        },
        flags,
        payload,
    )?;
    bytes.truncate(len);
    Some(Outbound {
        dst_ip: tcb.endpoints.remote_ip,
        bytes,
        sequence,
        flags,
    })
}

fn build(tcb: &TcpControlBlock, flags: u8, payload: &[u8]) -> Option<Outbound> {
    build_at(tcb, tcb.send_next, flags, payload)
}

fn retain_for_ack(tcb: &mut TcpControlBlock, sequence: u32, flags: u8, payload: &[u8]) {
    tcb.retransmit_queue.push(RetransmitEntry {
        sequence,
        flags,
        payload: payload.to_vec(),
        sent_at: now_ticks(),
        retransmitted: false,
    });
}

pub fn active_open(endpoints: Endpoints, isn: u32) -> Option<Outbound> {
    let mut tcb = TcpControlBlock {
        endpoints,
        state: TcpState::SynSent,
        send_next: isn,
        send_unacked: isn,
        recv_next: 0,
        send_window: 4096,
        receive: Vec::new(),
        connect_waiter: None,
        recv_waiter: None,
        retransmit_queue: Vec::new(),
        rtt: RttEstimator::new(),
    };
    let outbound = build(&tcb, FLAG_SYN, &[])?;
    retain_for_ack(&mut tcb, isn, FLAG_SYN, &[]);
    println!("[tcp] state CLOSED -> SYN_SENT; SYN seq={}", isn);
    *TCB.lock() = Some(tcb);
    Some(outbound)
}

pub fn register_connect_waiter(pid: Pid) -> bool {
    let mut slot = TCB.lock();
    let Some(tcb) = slot.as_mut() else {
        return false;
    };
    if tcb.state == TcpState::Established {
        return false;
    }
    tcb.connect_waiter = Some(pid);
    true
}

/// Queue as many payload bytes as the peer and fixed congestion windows allow.
pub fn send_data(payload: &[u8]) -> Option<(usize, Vec<Outbound>)> {
    let mut slot = TCB.lock();
    let tcb = slot.as_mut()?;
    if tcb.state != TcpState::Established || payload.is_empty() {
        return None;
    }
    let in_flight = tcb.send_next.wrapping_sub(tcb.send_unacked) as usize;
    let window = core::cmp::min(
        tcb.send_window as usize,
        CONGESTION_WINDOW_SEGMENTS * SEND_MSS,
    );
    let allowed = window.saturating_sub(in_flight).min(payload.len());
    if allowed == 0 {
        return Some((0, Vec::new()));
    }

    let mut outbound = Vec::new();
    for chunk in payload[..allowed].chunks(SEND_MSS) {
        let sequence = tcb.send_next;
        let segment = build_at(tcb, sequence, FLAG_PSH | FLAG_ACK, chunk)?;
        retain_for_ack(tcb, sequence, FLAG_PSH | FLAG_ACK, chunk);
        tcb.send_next = tcb.send_next.wrapping_add(chunk.len() as u32);
        println!(
            "[tcp] data tx len={} seq={} next={} in_flight={}",
            chunk.len(),
            sequence,
            tcb.send_next,
            tcb.send_next.wrapping_sub(tcb.send_unacked)
        );
        outbound.push(segment);
    }
    Some((allowed, outbound))
}

pub fn pop_byte() -> Option<u8> {
    let mut slot = TCB.lock();
    let tcb = slot.as_mut()?;
    if tcb.receive.is_empty() {
        None
    } else {
        Some(tcb.receive.remove(0))
    }
}

pub fn register_recv_waiter(pid: Pid) -> bool {
    let mut slot = TCB.lock();
    let Some(tcb) = slot.as_mut() else {
        return false;
    };
    if !tcb.receive.is_empty() {
        return false;
    }
    tcb.recv_waiter = Some(pid);
    true
}

/// Process one valid segment for the single active connection and return an ACK to transmit.
pub fn receive(src_ip: [u8; 4], dst_ip: [u8; 4], bytes: &[u8]) -> Option<Outbound> {
    let segment = TcpSegment::parse(bytes, src_ip, dst_ip)?;
    {
        let mut slot = TCB.lock();
        if let Some(tcb) = slot.as_mut() {
            let matches = src_ip == tcb.endpoints.remote_ip
                && dst_ip == tcb.endpoints.local_ip
                && segment.header.src_port == tcb.endpoints.remote_port
                && segment.header.dst_port == tcb.endpoints.local_port;
            if matches {
                if tcb.state == TcpState::SynSent
                    && segment.header.flags & (FLAG_SYN | FLAG_ACK) == (FLAG_SYN | FLAG_ACK)
                    && segment.header.acknowledgment == tcb.send_next.wrapping_add(1)
                {
                    tcb.send_next = tcb.send_next.wrapping_add(1);
                    acknowledge(tcb, segment.header.acknowledgment, now_ticks());
                    tcb.recv_next = segment.header.sequence.wrapping_add(1);
                    tcb.send_window = segment.header.window;
                    println!(
                        "[tcp] SYN-ACK seq={} ack={} matched",
                        segment.header.sequence, segment.header.acknowledgment
                    );
                    let outbound = build(tcb, FLAG_ACK, &[])?;
                    tcb.state = TcpState::Established;
                    println!(
                        "[tcp] final ACK seq={} ack={}; state SYN_SENT -> ESTABLISHED",
                        tcb.send_next, tcb.recv_next
                    );
                    if let Some(pid) = tcb.connect_waiter.take() {
                        crate::task::wake(pid);
                    }
                    return Some(outbound);
                }
                if tcb.state != TcpState::Established {
                    return None;
                }
                if segment.header.flags & FLAG_ACK != 0 {
                    acknowledge(tcb, segment.header.acknowledgment, now_ticks());
                    tcb.send_window = segment.header.window;
                }
                if !segment.payload.is_empty() && segment.header.sequence == tcb.recv_next {
                    tcb.receive.extend_from_slice(segment.payload);
                    tcb.recv_next = tcb.recv_next.wrapping_add(segment.payload.len() as u32);
                    println!(
                        "[tcp] data rx len={} next_ack={}",
                        segment.payload.len(),
                        tcb.recv_next
                    );
                    let outbound = build(tcb, FLAG_ACK, &[])?;
                    if let Some(pid) = tcb.recv_waiter.take() {
                        crate::task::wake(pid);
                    }
                    return Some(outbound);
                }
                return None;
            }
        }
    }
    receive_server(src_ip, dst_ip, &segment)
}

fn receive_server(src_ip: [u8; 4], dst_ip: [u8; 4], segment: &TcpSegment<'_>) -> Option<Outbound> {
    let key = ConnectionKey {
        local_ip: dst_ip,
        remote_ip: src_ip,
        local_port: segment.header.dst_port,
        remote_port: segment.header.src_port,
    };
    if let Some(socket) = CONNECTIONS.lock().get(&key).cloned() {
        let mut state = socket.inner.lock();
        let SocketState::Connection(connection) = &mut *state else {
            return None;
        };

        if connection.state == TcpState::SynReceived
            && segment.header.flags & FLAG_ACK != 0
            && segment.header.acknowledgment == connection.send_next
        {
            connection.send_unacked = segment.header.acknowledgment;
            connection.state = TcpState::Established;
            let listener = connection.listener.upgrade()?;
            drop(state);
            let waiter = {
                let mut listener_state = listener.inner.lock();
                let SocketState::Listener {
                    backlog,
                    accepted,
                    accept_waiter,
                    ..
                } = &mut *listener_state
                else {
                    return None;
                };
                if accepted.len() >= *backlog {
                    return None;
                }
                accepted.push_back(socket.clone());
                accept_waiter.take()
            };
            println!("[tcp] final ACK received; state SYN_RCVD -> ESTABLISHED");
            if let Some(pid) = waiter {
                crate::task::wake(pid);
            }
            return None;
        }

        if segment.header.flags & FLAG_ACK != 0 {
            connection.send_unacked = segment.header.acknowledgment;
            let old = connection.state;
            connection.state = match connection.state {
                TcpState::FinWait1 => TcpState::FinWait2,
                TcpState::Closing => TcpState::TimeWait,
                TcpState::LastAck => TcpState::Closed,
                other => other,
            };
            if connection.state != old {
                println!(
                    "[tcp] ACK received; state {:?} -> {:?}",
                    old, connection.state
                );
            }
            if connection.state == TcpState::Closed {
                if let Some(pid) = connection.close_waiter.take() {
                    crate::task::wake(pid);
                }
            }
        }

        if segment.header.sequence != connection.recv_next {
            return None;
        }
        if !segment.payload.is_empty() {
            connection.receive.extend(segment.payload.iter().copied());
            connection.recv_next = connection
                .recv_next
                .wrapping_add(segment.payload.len() as u32);
            println!(
                "[tcp] server data rx len={} next_ack={}",
                segment.payload.len(),
                connection.recv_next
            );
        }
        if segment.header.flags & FLAG_FIN != 0 {
            connection.recv_next = connection.recv_next.wrapping_add(1);
            connection.eof = true;
            let old = connection.state;
            connection.state = match old {
                TcpState::Established => TcpState::CloseWait,
                TcpState::FinWait1 => TcpState::Closing,
                TcpState::FinWait2 => TcpState::TimeWait,
                other => other,
            };
            println!(
                "[tcp] FIN received; state {:?} -> {:?}; ACK={}",
                old, connection.state, connection.recv_next
            );
        }
        if let Some(pid) = connection.recv_waiter.take() {
            crate::task::wake(pid);
        }
        if !segment.payload.is_empty() || segment.header.flags & FLAG_FIN != 0 {
            return build_server(connection, connection.send_next, FLAG_ACK, &[]);
        }
        return None;
    }

    if segment.header.flags & FLAG_SYN == 0 {
        return None;
    }
    let listener = LISTENERS
        .lock()
        .get(&segment.header.dst_port)
        .and_then(Weak::upgrade)?;
    let has_room = match &*listener.inner.lock() {
        SocketState::Listener {
            backlog, accepted, ..
        } => accepted.len() < *backlog,
        _ => false,
    };
    if !has_room {
        return None;
    }

    let isn = (now_ticks() as u32)
        .wrapping_mul(1_103_515_245)
        .wrapping_add(0x33ec_0001);
    let connection = ServerConnection {
        endpoints: Endpoints {
            local_ip: dst_ip,
            remote_ip: src_ip,
            local_port: segment.header.dst_port,
            remote_port: segment.header.src_port,
        },
        state: TcpState::SynReceived,
        send_next: isn.wrapping_add(1),
        send_unacked: isn,
        recv_next: segment.header.sequence.wrapping_add(1),
        receive: VecDeque::new(),
        eof: false,
        recv_waiter: None,
        close_waiter: None,
        listener: Arc::downgrade(&listener),
    };
    let reply = build_server(&connection, isn, FLAG_SYN | FLAG_ACK, &[])?;
    let socket = Arc::new(TcpSocket {
        inner: IrqMutex::new(SocketState::Connection(connection)),
    });
    CONNECTIONS.lock().insert(key, socket);
    println!(
        "[tcp] SYN received on port {}; state LISTEN -> SYN_RCVD; SYN-ACK seq={}",
        segment.header.dst_port, isn
    );
    Some(reply)
}

fn acknowledge(tcb: &mut TcpControlBlock, acknowledgment: u32, now: usize) {
    if !sequence_after(acknowledgment, tcb.send_unacked)
        || sequence_after(acknowledgment, tcb.send_next)
    {
        return;
    }
    let mut sample = None;
    let mut acknowledged_retransmission = false;
    tcb.retransmit_queue.retain(|entry| {
        if !sequence_after(entry.sequence_end(), acknowledgment) {
            if entry.retransmitted {
                acknowledged_retransmission = true;
            } else {
                sample = Some(now.saturating_sub(entry.sent_at));
            }
            false
        } else {
            true
        }
    });
    tcb.send_unacked = acknowledgment;
    println!(
        "[tcp] ACK={} send_unacked={} retransmit_queue={}",
        acknowledgment,
        tcb.send_unacked,
        tcb.retransmit_queue.len()
    );
    if !acknowledged_retransmission {
        if let Some(sample_ticks) = sample {
            tcb.rtt.record_sample(sample_ticks);
        }
    } else {
        println!("[tcp] Karn: skipped ambiguous RTT sample after retransmission");
    }
}

/// Compare wrapping TCP sequence numbers within the RFC 9293 half-space.
fn sequence_after(left: u32, right: u32) -> bool {
    (left.wrapping_sub(right) as i32) > 0
}

/// Return the oldest expired segment for transmission.
///
/// The network task calls this once per timer wake. Karn's algorithm marks
/// the entry so its eventual ACK cannot produce an ambiguous RTT sample.
pub fn poll_retransmit(now: usize) -> Option<Outbound> {
    let mut slot = TCB.lock();
    let tcb = slot.as_mut()?;
    let entry = tcb.retransmit_queue.first()?;
    if now.saturating_sub(entry.sent_at) < tcb.rtt.current_rto_ticks {
        return None;
    }
    let sequence = entry.sequence;
    let flags = entry.flags;
    let payload = entry.payload.clone();
    let previous_rto = tcb.rtt.current_rto_ticks;
    tcb.retransmit_queue[0].sent_at = now;
    tcb.retransmit_queue[0].retransmitted = true;
    tcb.rtt.back_off();
    println!(
        "[tcp] RTO expired seq={} len={}; retransmit; rto {} -> {} ticks",
        sequence,
        payload.len(),
        previous_rto,
        tcb.rtt.current_rto_ticks
    );
    build_at(tcb, sequence, flags, &payload)
}

pub fn state() -> TcpState {
    TCB.lock()
        .as_ref()
        .map(|t| t.state)
        .unwrap_or(TcpState::Closed)
}
