//! Small network protocol helpers.

pub mod arp;
pub mod checksum;
pub mod ethernet;
pub mod icmp;
pub mod ipv4;
pub mod socket;
pub mod tcp;
pub mod udp;

use crate::irq_mutex::IrqMutex;

// Fixed values used by protocol unit tests and the pre-DHCP test fixtures.
// Packet transmission reads CONFIG through source_ip() and gateway_ip().
pub const OUR_IP: [u8; 4] = [10, 0, 2, 15];
pub const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkConfig {
    pub address: [u8; 4],
    pub subnet_mask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns: [u8; 4],
    pub lease_seconds: u32,
}

static CONFIG: IrqMutex<Option<NetworkConfig>> = IrqMutex::new(None);

pub fn configure(config: NetworkConfig) {
    *CONFIG.lock() = Some(config);
}

pub fn config() -> Option<NetworkConfig> {
    *CONFIG.lock()
}

pub fn source_ip() -> [u8; 4] {
    config().map(|value| value.address).unwrap_or([0; 4])
}

pub fn gateway_ip() -> Option<[u8; 4]> {
    config().map(|value| value.gateway)
}

pub fn accepts_destination(address: [u8; 4]) -> bool {
    address == [255; 4] || config().map(|value| value.address == address).unwrap_or(false)
}
