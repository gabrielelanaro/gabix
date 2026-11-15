//! Host test of the same frame-driven lease session used by ARM targets.
#[path = "../targets/armv7/src/network.rs"]
mod network;
use gabix_kernel::device::{EthernetFrame, FrameDevice};
use gabix_kernel::net::stack::NetworkInterface;
use network::{DhcpSession, LeaseState, NetworkEvent};
const MAC: [u8; 6] = [2, 0, 0, 0, 0, 1];
const SERVER_MAC: [u8; 6] = [2, 0, 0, 0, 0, 2];
const ADDRESS: [u8; 4] = [192, 168, 178, 42];
const SERVER: [u8; 4] = [192, 168, 178, 1];
struct Device {
    sent: Vec<Vec<u8>>,
}
impl FrameDevice for Device {
    fn mac_address(&self) -> [u8; 6] {
        MAC
    }
    fn transmit(&mut self, frame: &[u8]) -> bool {
        self.sent.push(frame.to_vec());
        true
    }
    fn receive(&mut self) -> Option<EthernetFrame> {
        None
    }
}
fn reply(kind: u8, xid: u32, address: [u8; 4]) -> Vec<u8> {
    let mut packet = [0u8; 320];
    packet[0] = 2;
    packet[1] = 1;
    packet[2] = 6;
    packet[4..8].copy_from_slice(&xid.to_be_bytes());
    packet[16..20].copy_from_slice(&address);
    packet[28..34].copy_from_slice(&MAC);
    packet[236..240].copy_from_slice(&[99, 130, 83, 99]);
    let options = [
        53, 1, kind, 54, 4, 192, 168, 178, 1, 1, 4, 255, 255, 255, 0, 6, 8, 192, 168, 178, 1, 1, 1,
        1, 1, 51, 4, 0, 0, 0, 60, 58, 4, 0, 0, 0, 30, 59, 4, 0, 0, 0, 50, 255,
    ];
    packet[240..240 + options.len()].copy_from_slice(&options);
    let server = NetworkInterface::new(SERVER_MAC, SERVER);
    let mut out = [0u8; 1514];
    let n = server
        .write_udp([255; 6], [255; 4], 67, 68, &packet, &mut out)
        .unwrap();
    out[..n].to_vec()
}
fn options(packet: &[u8]) -> Vec<u8> {
    let mut codes = Vec::new();
    let mut i = 240;
    while i < packet.len() {
        let code = packet[i];
        i += 1;
        if code == 255 {
            break;
        }
        if code == 0 {
            continue;
        }
        codes.push(code);
        i += 1 + packet[i] as usize;
    }
    codes
}
#[test]
fn test_lease_renews_rebinds_and_expires() {
    let mut session = DhcpSession::new(MAC, 7);
    let mut device = Device { sent: Vec::new() };
    session.poll(&mut device, 0, true);
    session.poll(&mut device, 4, true);
    assert_eq!(session.attempts, 2);
    session.handle_frame(&mut device, &reply(2, 7, ADDRESS));
    assert_eq!(session.state, LeaseState::Requesting);
    assert!(matches!(
        session.handle_frame(&mut device, &reply(5, 7, ADDRESS)),
        NetworkEvent::Configured
    ));
    assert_eq!(session.lease.unwrap().dns, SERVER);
    session.poll(&mut device, 34, true);
    assert_eq!(session.state, LeaseState::Renewing);
    let server = NetworkInterface::new(SERVER_MAC, SERVER);
    let request = server.receive_udp(device.sent.last().unwrap()).unwrap();
    assert_eq!(&request.payload[12..16], &ADDRESS);
    assert_eq!(request.source_ip, ADDRESS);
    assert!(!options(request.payload).contains(&50));
    assert!(!options(request.payload).contains(&54));
    session.handle_frame(&mut device, &reply(5, 7, [0; 4]));
    assert_eq!(session.lease_remaining(), 60);
    session.poll(&mut device, 64, true);
    assert_eq!(session.state, LeaseState::Renewing);
    session.poll(&mut device, 84, true);
    assert_eq!(session.state, LeaseState::Rebinding);
    let request = server.receive_udp(device.sent.last().unwrap()).unwrap();
    assert_eq!(request.destination_ip, [255; 4]);
    session.poll(&mut device, 94, true);
    assert_eq!(session.state, LeaseState::Selecting);
    assert_eq!(session.address(), [0; 4]);
    session.handle_frame(&mut device, &reply(5, 7, ADDRESS));
    assert!(!session.is_configured());
    session.handle_frame(&mut device, &reply(2, 8, ADDRESS));
    session.handle_frame(&mut device, &reply(5, 8, ADDRESS));
    assert!(session.is_configured());
}
#[test]
fn test_nak_and_missing_ack_restart_discovery() {
    let mut session = DhcpSession::new(MAC, 7);
    let mut device = Device { sent: Vec::new() };
    session.poll(&mut device, 0, true);
    session.handle_frame(&mut device, &reply(2, 7, ADDRESS));
    session.handle_frame(&mut device, &reply(5, 7, [192, 168, 178, 99]));
    assert!(!session.is_configured());
    session.handle_frame(&mut device, &reply(6, 7, [0; 4]));
    assert_eq!(session.state, LeaseState::Selecting);
    session.handle_frame(&mut device, &reply(2, 8, ADDRESS));
    for now in [4, 8, 16, 32] {
        session.poll(&mut device, now, true);
    }
    assert_eq!(session.state, LeaseState::Selecting);
    let count = device.sent.len();
    session.poll(&mut device, 100, false);
    assert_eq!(device.sent.len(), count);
    session.poll(&mut device, 101, true);
    assert!(device.sent.len() > count);
}
