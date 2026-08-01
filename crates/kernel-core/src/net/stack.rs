//! Driver-independent Ethernet, ARP, IPv4, ICMP, and UDP handling.

use super::arp::{ArpPacket, ARP_PACKET_LEN, OPER_REQUEST};
use super::ethernet::{EthernetHeader, ETHERNET_HEADER_LEN, ETHERTYPE_ARP, ETHERTYPE_IPV4};
use super::icmp::{self, EchoMessage, ECHO_CODE, ECHO_REQUEST};
use super::ipv4::{self, Ipv4Header, IPV4_HEADER_LEN};
use super::udp::{self, UdpDatagram, UDP_HEADER_LEN};

pub const ETHERNET_MTU: usize = 1500;
pub const MAX_FRAME_LEN: usize = ETHERNET_HEADER_LEN + ETHERNET_MTU;

pub struct NetworkInterface {
    mac: [u8; 6],
    address: [u8; 4],
}

pub struct ReceivedUdp<'a> {
    pub source_mac: [u8; 6],
    pub source_ip: [u8; 4],
    pub destination_ip: [u8; 4],
    pub source_port: u16,
    pub destination_port: u16,
    pub payload: &'a [u8],
}

impl NetworkInterface {
    pub const fn new(mac: [u8; 6], address: [u8; 4]) -> Self {
        Self { mac, address }
    }

    pub const fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    pub const fn address(&self) -> [u8; 4] {
        self.address
    }

    pub fn configure(&mut self, address: [u8; 4]) {
        self.address = address;
    }

    pub fn reply(&self, frame: &[u8], out: &mut [u8]) -> Option<usize> {
        let ethernet = EthernetHeader::parse(frame)?;
        if ethernet.dst != self.mac && ethernet.dst != [0xff; 6] {
            return None;
        }
        let payload = frame.get(ETHERNET_HEADER_LEN..)?;

        if ethernet.ethertype == ETHERTYPE_ARP {
            let request = ArpPacket::parse(payload)?;
            if request.is_ipv4_over_ethernet()
                && request.oper == OPER_REQUEST
                && request.tpa == self.address
            {
                let reply = ArpPacket::reply_to(&request, self.mac, self.address);
                return self.write_ethernet(ethernet.src, ETHERTYPE_ARP, &reply.to_bytes(), out);
            }
            return None;
        }

        if ethernet.ethertype != ETHERTYPE_IPV4 {
            return None;
        }
        let ip = Ipv4Header::parse(payload)?;
        if ip.dst_ip != self.address && ip.dst_ip != [255; 4] {
            return None;
        }
        let ip_payload = payload.get(IPV4_HEADER_LEN..ip.total_len as usize)?;

        if ip.protocol == ipv4::PROTOCOL_ICMP {
            let request = EchoMessage::parse(ip_payload)?;
            if request.message_type != ECHO_REQUEST || request.code != ECHO_CODE {
                return None;
            }
            let mut reply = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN];
            let length = icmp::write_echo_reply(
                &mut reply,
                request.identifier,
                request.sequence,
                request.payload,
            )?;
            return self.write_ipv4(
                ethernet.src,
                ip.src_ip,
                ipv4::PROTOCOL_ICMP,
                &reply[..length],
                out,
            );
        }

        if ip.protocol == ipv4::PROTOCOL_UDP {
            let datagram = UdpDatagram::parse(ip_payload, ip.src_ip, ip.dst_ip)?;
            if datagram.header.dst_port == udp::ECHO_PORT {
                return self.write_udp(
                    ethernet.src,
                    ip.src_ip,
                    udp::ECHO_PORT,
                    datagram.header.src_port,
                    datagram.payload,
                    out,
                );
            }
        }
        None
    }

    pub fn receive_udp<'a>(&self, frame: &'a [u8]) -> Option<ReceivedUdp<'a>> {
        let ethernet = EthernetHeader::parse(frame)?;
        if ethernet.ethertype != ETHERTYPE_IPV4
            || (ethernet.dst != self.mac && ethernet.dst != [0xff; 6])
        {
            return None;
        }
        let bytes = frame.get(ETHERNET_HEADER_LEN..)?;
        let ip = Ipv4Header::parse(bytes)?;
        if ip.protocol != ipv4::PROTOCOL_UDP || (ip.dst_ip != self.address && ip.dst_ip != [255; 4])
        {
            return None;
        }
        let payload = bytes.get(IPV4_HEADER_LEN..ip.total_len as usize)?;
        let udp = UdpDatagram::parse(payload, ip.src_ip, ip.dst_ip)?;
        Some(ReceivedUdp {
            source_mac: ethernet.src,
            source_ip: ip.src_ip,
            destination_ip: ip.dst_ip,
            source_port: udp.header.src_port,
            destination_port: udp.header.dst_port,
            payload: udp.payload,
        })
    }

    pub fn write_udp(
        &self,
        destination_mac: [u8; 6],
        destination_ip: [u8; 4],
        source_port: u16,
        destination_port: u16,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        self.write_udp_from(
            self.address,
            destination_mac,
            destination_ip,
            source_port,
            destination_port,
            payload,
            out,
        )
    }

    pub fn write_udp_from(
        &self,
        source_ip: [u8; 4],
        destination_mac: [u8; 6],
        destination_ip: [u8; 4],
        source_port: u16,
        destination_port: u16,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        let mut datagram = [0u8; ETHERNET_MTU - IPV4_HEADER_LEN];
        let length = udp::write_datagram(
            &mut datagram,
            source_ip,
            destination_ip,
            source_port,
            destination_port,
            payload,
        )?;
        self.write_ipv4_from(
            source_ip,
            destination_mac,
            destination_ip,
            ipv4::PROTOCOL_UDP,
            &datagram[..length],
            out,
        )
    }

    pub fn write_arp_request(&self, target_ip: [u8; 4], out: &mut [u8]) -> Option<usize> {
        let request = ArpPacket::request(self.mac, self.address, target_ip);
        self.write_ethernet([0xff; 6], ETHERTYPE_ARP, &request.to_bytes(), out)
    }

    fn write_ipv4(
        &self,
        destination_mac: [u8; 6],
        destination_ip: [u8; 4],
        protocol: u8,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        self.write_ipv4_from(
            self.address,
            destination_mac,
            destination_ip,
            protocol,
            payload,
            out,
        )
    }

    fn write_ipv4_from(
        &self,
        source_ip: [u8; 4],
        destination_mac: [u8; 6],
        destination_ip: [u8; 4],
        protocol: u8,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        if payload.len() > ETHERNET_MTU - IPV4_HEADER_LEN {
            return None;
        }
        let header = Ipv4Header::build(protocol, source_ip, destination_ip, payload.len());
        let mut packet = [0u8; ETHERNET_MTU];
        packet[..IPV4_HEADER_LEN].copy_from_slice(&header);
        packet[IPV4_HEADER_LEN..IPV4_HEADER_LEN + payload.len()].copy_from_slice(payload);
        self.write_ethernet(
            destination_mac,
            ETHERTYPE_IPV4,
            &packet[..IPV4_HEADER_LEN + payload.len()],
            out,
        )
    }

    fn write_ethernet(
        &self,
        destination_mac: [u8; 6],
        ethertype: u16,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        let length = ETHERNET_HEADER_LEN.checked_add(payload.len())?;
        if payload.len() > ETHERNET_MTU || out.len() < length {
            return None;
        }
        let header = EthernetHeader {
            dst: destination_mac,
            src: self.mac,
            ethertype,
        };
        out[..ETHERNET_HEADER_LEN].copy_from_slice(&header.to_bytes());
        out[ETHERNET_HEADER_LEN..length].copy_from_slice(payload);
        Some(length)
    }
}

const _: () = assert!(ARP_PACKET_LEN <= ETHERNET_MTU);
const _: () = assert!(UDP_HEADER_LEN <= ETHERNET_MTU - IPV4_HEADER_LEN);
