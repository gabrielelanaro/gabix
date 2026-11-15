//! SMSC USB/Ethernet framing, matching Linux drivers/net/usb/smsc95xx.h.
//! TX: two little-endian command words, then the Ethernet frame bytes.
//! RX: one little-endian status word, frame bytes, then the four-byte Ethernet
//! frame check sequence (FCS). The network stack receives the frame without FCS.

const TX_FIRST_SEGMENT: u32 = 0x2000;
const TX_LAST_SEGMENT: u32 = 0x1000;
const TX_LENGTH_MASK: usize = 0x07ff;
const RX_LENGTH_SHIFT: u32 = 16;
const SMSC_VENDOR_ID: u16 = 0x0424;
const SUPPORTED_PRODUCT_IDS: [u16; 3] = [0xec00, 0x9904, 0x9909];
const COMMAND_WORD_BYTES: usize = 4;
const RX_LENGTH_MASK: u32 = 0x3fff_0000;
const RX_ERROR: u32 = 0x0000_8000;
const ETHERNET_FCS_LEN: usize = 4;

pub const TX_HEADER_LEN: usize = 8;
pub const RX_HEADER_LEN: usize = 4;

pub const fn is_lan9512(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == SMSC_VENDOR_ID
        && (product_id == SUPPORTED_PRODUCT_IDS[0]
            || product_id == SUPPORTED_PRODUCT_IDS[1]
            || product_id == SUPPORTED_PRODUCT_IDS[2])
}

pub fn write_tx_frame(frame: &[u8], out: &mut [u8]) -> Option<usize> {
    if frame.is_empty() || frame.len() > TX_LENGTH_MASK {
        return None;
    }
    let total = TX_HEADER_LEN.checked_add(frame.len())?;
    if total > out.len() {
        return None;
    }

    let length = frame.len() as u32;
    // One USB buffer contains the complete frame, so both segment flags are set.
    let command_a = length | TX_FIRST_SEGMENT | TX_LAST_SEGMENT;
    out[..COMMAND_WORD_BYTES].copy_from_slice(&command_a.to_le_bytes());
    out[COMMAND_WORD_BYTES..TX_HEADER_LEN].copy_from_slice(&length.to_le_bytes());
    out[TX_HEADER_LEN..total].copy_from_slice(frame);
    Some(total)
}

pub fn read_rx_frame(bytes: &[u8]) -> Option<&[u8]> {
    let status = u32::from_le_bytes(bytes.get(..RX_HEADER_LEN)?.try_into().ok()?);
    if status & RX_ERROR != 0 {
        return None;
    }
    let received_len = ((status & RX_LENGTH_MASK) >> RX_LENGTH_SHIFT) as usize;
    if received_len <= ETHERNET_FCS_LEN {
        return None;
    }
    let received_end = RX_HEADER_LEN.checked_add(received_len)?;
    if received_end > bytes.len() {
        return None;
    }
    Some(&bytes[RX_HEADER_LEN..received_end - ETHERNET_FCS_LEN])
}
