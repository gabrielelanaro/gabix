//! Shared ARP packet format with the x86 kernel's runtime cache.

extern crate alloc;

use crate::irq_mutex::IrqMutex;
use alloc::collections::BTreeMap;
use lazy_static::lazy_static;

pub use gabix_kernel::net::arp::*;

lazy_static! {
    static ref ARP_CACHE: IrqMutex<BTreeMap<[u8; 4], [u8; 6]>> = IrqMutex::new(BTreeMap::new());
}

pub fn lookup(ip: [u8; 4]) -> Option<[u8; 6]> {
    ARP_CACHE.lock().get(&ip).copied()
}

pub fn merge(packet: &ArpPacket) -> Option<[u8; 6]> {
    if !packet.is_ipv4_over_ethernet() {
        return None;
    }
    ARP_CACHE.lock().insert(packet.spa, packet.sha)
}

pub fn cache_len() -> usize {
    ARP_CACHE.lock().len()
}
