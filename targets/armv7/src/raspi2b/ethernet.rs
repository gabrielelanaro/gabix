//! Select one USB network adapter and expose the shared Ethernet-frame API.
//! The supported USB identities are explicit; this is not generic class probing.

use super::lan9512::Lan9512Ethernet;
use super::rndis::RndisEthernet;
use super::usb::{Dwc2, UsbDevice};
use gabix_kernel::device::{EthernetFrame, FrameDevice};
use gabix_kernel::net::smsc95xx::is_lan9512;

#[cfg(feature = "pi-wifi")]
const RTL8188CU_USB_ID: (u16, u16) = (0x0bda, 0x8176);
const QEMU_RNDIS_USB_ID: (u16, u16) = (0x0525, 0xa4a2);

pub enum UsbEthernet {
    Lan9512(Lan9512Ethernet),
    Rndis(RndisEthernet),
    #[cfg(feature = "pi-wifi")]
    Wifi(super::wifi_ethernet::WifiEthernet),
}

impl UsbEthernet {
    pub unsafe fn configure(host: Dwc2, device: UsbDevice) -> Option<Self> {
        #[cfg(feature = "pi-wifi")]
        if (device.vendor_id, device.product_id) == RTL8188CU_USB_ID {
            return super::wifi_ethernet::WifiEthernet::configure(
                host,
                device,
                env!("GABIX_WIFI_SSID").as_bytes(),
                include_bytes!(env!("GABIX_WIFI_PMK_FILE")),
                include_bytes!(env!("GABIX_WIFI_FIRMWARE")),
            )
            .ok()
            .map(Self::Wifi);
        }
        if is_lan9512(device.vendor_id, device.product_id) {
            Lan9512Ethernet::configure(host, device).map(Self::Lan9512)
        } else if (device.vendor_id, device.product_id) == QEMU_RNDIS_USB_ID {
            RndisEthernet::configure(host, device).map(Self::Rndis)
        } else {
            None
        }
    }

    /// None means link status is unavailable; RNDIS has no status query here.
    pub unsafe fn link_up(&mut self) -> Option<bool> {
        match self {
            Self::Lan9512(device) => device.link_up(),
            Self::Rndis(_) => None,
            #[cfg(feature = "pi-wifi")]
            Self::Wifi(device) => Some(device.link_up()),
        }
    }

    pub fn is_wifi(&self) -> bool {
        match self {
            #[cfg(feature = "pi-wifi")]
            Self::Wifi(_) => true,
            _ => false,
        }
    }

    #[cfg(feature = "pi-wifi")]
    pub fn take_wifi_event(&mut self) -> Option<super::wifi_ethernet::Event> {
        match self {
            Self::Wifi(device) => device.take_event(),
            _ => None,
        }
    }
}

unsafe impl Send for UsbEthernet {}

impl FrameDevice for UsbEthernet {
    fn mac_address(&self) -> [u8; 6] {
        match self {
            Self::Lan9512(device) => device.mac_address(),
            Self::Rndis(device) => device.mac_address(),
            #[cfg(feature = "pi-wifi")]
            Self::Wifi(device) => device.mac_address(),
        }
    }

    fn transmit(&mut self, frame: &[u8]) -> bool {
        match self {
            Self::Lan9512(device) => device.transmit(frame),
            Self::Rndis(device) => device.transmit(frame),
            #[cfg(feature = "pi-wifi")]
            Self::Wifi(device) => device.transmit(frame),
        }
    }

    fn receive(&mut self) -> Option<EthernetFrame> {
        match self {
            Self::Lan9512(device) => device.receive(),
            Self::Rndis(device) => device.receive(),
            #[cfg(feature = "pi-wifi")]
            Self::Wifi(device) => device.receive(),
        }
    }
}
