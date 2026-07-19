//! Minimal active-open TCP used by Lesson 31.

use alloc::vec::Vec;
use crate::irq_mutex::IrqMutex;
use crate::net::checksum;
use crate::process::Pid;
use crate::println;

pub const PROTOCOL_TCP: u8 = 6;
pub const TCP_HEADER_LEN: usize = 20;
pub const FLAG_FIN: u8 = 0x01;
pub const FLAG_SYN: u8 = 0x02;
pub const FLAG_RST: u8 = 0x04;
pub const FLAG_PSH: u8 = 0x08;
pub const FLAG_ACK: u8 = 0x10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpState { Closed, SynSent, Established }

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

pub struct TcpSegment<'a> { pub header: TcpHeader, pub payload: &'a [u8] }

impl<'a> TcpSegment<'a> {
    pub fn parse(bytes: &'a [u8], src_ip: [u8; 4], dst_ip: [u8; 4]) -> Option<Self> {
        if bytes.len() < TCP_HEADER_LEN || tcp_checksum(src_ip, dst_ip, bytes) != 0 { return None; }
        let header_len = ((bytes[12] >> 4) as usize) * 4;
        if header_len < TCP_HEADER_LEN || header_len > bytes.len() { return None; }
        Some(Self {
            header: TcpHeader {
                src_port: u16::from_be_bytes([bytes[0], bytes[1]]),
                dst_port: u16::from_be_bytes([bytes[2], bytes[3]]),
                sequence: u32::from_be_bytes(bytes[4..8].try_into().ok()?),
                acknowledgment: u32::from_be_bytes(bytes[8..12].try_into().ok()?),
                flags: bytes[13],
                window: u16::from_be_bytes([bytes[14], bytes[15]]),
                header_len,
            },
            payload: &bytes[header_len..],
        })
    }
}

/// Write a TCP segment. SYN segments include the four-byte MSS option.
pub fn write_segment(out: &mut [u8], endpoints: Endpoints, sequence: u32,
    acknowledgment: u32, flags: u8, payload: &[u8]) -> Option<usize> {
    let header_len = if flags & FLAG_SYN != 0 { 24 } else { TCP_HEADER_LEN };
    let length = header_len.checked_add(payload.len())?;
    if length > u16::MAX as usize || out.len() < length { return None; }
    out[..length].fill(0);
    out[0..2].copy_from_slice(&endpoints.local_port.to_be_bytes());
    out[2..4].copy_from_slice(&endpoints.remote_port.to_be_bytes());
    out[4..8].copy_from_slice(&sequence.to_be_bytes());
    out[8..12].copy_from_slice(&acknowledgment.to_be_bytes());
    out[12] = ((header_len / 4) as u8) << 4;
    out[13] = flags;
    out[14..16].copy_from_slice(&4096u16.to_be_bytes());
    if header_len == 24 { out[20..24].copy_from_slice(&[2, 4, 0x05, 0xb4]); }
    out[header_len..length].copy_from_slice(payload);
    let sum = tcp_checksum(endpoints.local_ip, endpoints.remote_ip, &out[..length]);
    out[16..18].copy_from_slice(&sum.to_be_bytes());
    Some(length)
}

fn tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], bytes: &[u8]) -> u16 {
    let mut pseudo = [0u8; 12];
    pseudo[0..4].copy_from_slice(&src_ip); pseudo[4..8].copy_from_slice(&dst_ip);
    pseudo[9] = PROTOCOL_TCP; pseudo[10..12].copy_from_slice(&(bytes.len() as u16).to_be_bytes());
    checksum::internet_checksum_parts(&[&pseudo, bytes])
}

#[derive(Clone, Copy)]
pub struct Endpoints { pub local_ip: [u8; 4], pub remote_ip: [u8; 4], pub local_port: u16, pub remote_port: u16 }

struct TcpControlBlock {
    endpoints: Endpoints, state: TcpState, send_next: u32, send_unacked: u32,
    recv_next: u32, recv_window: u16, receive: Vec<u8>, connect_waiter: Option<Pid>,
    recv_waiter: Option<Pid>,
}

static TCB: IrqMutex<Option<TcpControlBlock>> = IrqMutex::new(None);

pub struct Outbound { pub dst_ip: [u8; 4], pub bytes: Vec<u8> }

fn build(tcb: &TcpControlBlock, flags: u8, payload: &[u8]) -> Option<Outbound> {
    let mut bytes = Vec::new(); bytes.resize(24 + payload.len(), 0);
    let len = write_segment(&mut bytes, tcb.endpoints, tcb.send_next,
        if flags & FLAG_ACK != 0 { tcb.recv_next } else { 0 }, flags, payload)?;
    bytes.truncate(len); Some(Outbound { dst_ip: tcb.endpoints.remote_ip, bytes })
}

pub fn active_open(endpoints: Endpoints, isn: u32) -> Option<Outbound> {
    let tcb = TcpControlBlock { endpoints, state: TcpState::SynSent, send_next: isn,
        send_unacked: isn, recv_next: 0, recv_window: 4096, receive: Vec::new(),
        connect_waiter: None, recv_waiter: None };
    let outbound = build(&tcb, FLAG_SYN, &[])?;
    println!("[tcp] state CLOSED -> SYN_SENT; SYN seq={}", isn);
    *TCB.lock() = Some(tcb); Some(outbound)
}

pub fn register_connect_waiter(pid: Pid) -> bool {
    let mut slot = TCB.lock(); let Some(tcb) = slot.as_mut() else { return false };
    if tcb.state == TcpState::Established { return false; }
    tcb.connect_waiter = Some(pid); true
}

pub fn send_data(payload: &[u8]) -> Option<Outbound> {
    let mut slot = TCB.lock(); let tcb = slot.as_mut()?;
    if tcb.state != TcpState::Established || payload.is_empty() { return None; }
    let seq = tcb.send_next;
    let outbound = build(tcb, FLAG_PSH | FLAG_ACK, payload)?;
    tcb.send_next = tcb.send_next.wrapping_add(payload.len() as u32);
    println!("[tcp] data tx len={} seq={} next={}", payload.len(), seq, tcb.send_next);
    Some(outbound)
}

pub fn pop_byte() -> Option<u8> {
    let mut slot = TCB.lock(); let tcb = slot.as_mut()?;
    if tcb.receive.is_empty() { None } else { Some(tcb.receive.remove(0)) }
}

pub fn register_recv_waiter(pid: Pid) -> bool {
    let mut slot = TCB.lock(); let Some(tcb) = slot.as_mut() else { return false };
    if !tcb.receive.is_empty() { return false; }
    tcb.recv_waiter = Some(pid); true
}

/// Process one valid segment for the single active connection and return an ACK to transmit.
pub fn receive(src_ip: [u8; 4], dst_ip: [u8; 4], bytes: &[u8]) -> Option<Outbound> {
    let segment = TcpSegment::parse(bytes, src_ip, dst_ip)?;
    let mut slot = TCB.lock(); let tcb = slot.as_mut()?;
    if src_ip != tcb.endpoints.remote_ip || dst_ip != tcb.endpoints.local_ip
        || segment.header.src_port != tcb.endpoints.remote_port
        || segment.header.dst_port != tcb.endpoints.local_port { return None; }

    if tcb.state == TcpState::SynSent && segment.header.flags & (FLAG_SYN | FLAG_ACK) == (FLAG_SYN | FLAG_ACK)
        && segment.header.acknowledgment == tcb.send_next.wrapping_add(1) {
        tcb.send_next = tcb.send_next.wrapping_add(1); tcb.send_unacked = tcb.send_next;
        tcb.recv_next = segment.header.sequence.wrapping_add(1); tcb.recv_window = segment.header.window;
        println!("[tcp] SYN-ACK seq={} ack={} matched", segment.header.sequence, segment.header.acknowledgment);
        let outbound = build(tcb, FLAG_ACK, &[])?;
        tcb.state = TcpState::Established;
        println!("[tcp] final ACK seq={} ack={}; state SYN_SENT -> ESTABLISHED", tcb.send_next, tcb.recv_next);
        if let Some(pid) = tcb.connect_waiter.take() { crate::task::wake(pid); }
        return Some(outbound);
    }
    if tcb.state != TcpState::Established { return None; }
    if segment.header.acknowledgment > tcb.send_unacked && segment.header.acknowledgment <= tcb.send_next {
        tcb.send_unacked = segment.header.acknowledgment;
    }
    if !segment.payload.is_empty() && segment.header.sequence == tcb.recv_next {
        tcb.receive.extend_from_slice(segment.payload);
        tcb.recv_next = tcb.recv_next.wrapping_add(segment.payload.len() as u32);
        println!("[tcp] data rx len={} next_ack={}", segment.payload.len(), tcb.recv_next);
        let outbound = build(tcb, FLAG_ACK, &[])?;
        if let Some(pid) = tcb.recv_waiter.take() { crate::task::wake(pid); }
        return Some(outbound);
    }
    None
}

pub fn state() -> TcpState { TCB.lock().as_ref().map(|t| t.state).unwrap_or(TcpState::Closed) }
