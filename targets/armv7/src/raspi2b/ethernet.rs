use super::lan9512::Lan9512Ethernet;
use super::rndis::RndisEthernet;
use super::usb::{Dwc2, UsbDevice};
use gabix_kernel::device::{EthernetFrame, FrameDevice};
use gabix_kernel::net::smsc95xx::is_lan9512;

pub enum UsbEthernet {
    Lan9512(Lan9512Ethernet),
    Rndis(RndisEthernet),
}

impl UsbEthernet {
    pub unsafe fn configure(host: Dwc2, device: UsbDevice) -> Option<Self> {
        if is_lan9512(device.vendor_id, device.product_id) {
            Lan9512Ethernet::configure(host, device).map(Self::Lan9512)
        } else if device.vendor_id == 0x0525 && device.product_id == 0xa4a2 {
            RndisEthernet::configure(host, device).map(Self::Rndis)
        } else {
            None
        }
    }

    pub unsafe fn link_up(&mut self) -> Option<bool> {
        match self {
            Self::Lan9512(device) => device.link_up(),
            Self::Rndis(_) => None,
        }
    }
}

unsafe impl Send for UsbEthernet {}

impl FrameDevice for UsbEthernet {
    fn mac_address(&self) -> [u8; 6] {
        match self {
            Self::Lan9512(device) => device.mac_address(),
            Self::Rndis(device) => device.mac_address(),
        }
    }

    fn transmit(&mut self, frame: &[u8]) -> bool {
        match self {
            Self::Lan9512(device) => device.transmit(frame),
            Self::Rndis(device) => device.transmit(frame),
        }
    }

    fn receive(&mut self) -> Option<EthernetFrame> {
        match self {
            Self::Lan9512(device) => device.receive(),
            Self::Rndis(device) => device.receive(),
        }
    }
}
