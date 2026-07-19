//! Small network protocol helpers.

pub mod arp;
pub mod checksum;
pub mod ethernet;
pub mod icmp;
pub mod ipv4;
pub mod socket;
pub mod udp;

pub const OUR_IP: [u8; 4] = [10, 0, 2, 15];
pub const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
