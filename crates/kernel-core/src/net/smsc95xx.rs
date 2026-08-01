const TX_FIRST_SEGMENT: u32 = 0x2000;
const TX_LAST_SEGMENT: u32 = 0x1000;
const TX_LENGTH_MASK: usize = 0x07ff;
const RX_LENGTH_MASK: u32 = 0x3fff_0000;
const RX_ERROR: u32 = 0x0000_8000;
const ETHERNET_FCS_LEN: usize = 4;

pub const TX_HEADER_LEN: usize = 8;
pub const RX_HEADER_LEN: usize = 4;

pub const fn is_lan9512(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == 0x0424 && matches!(product_id, 0xec00 | 0x9904 | 0x9909)
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
    let command_a = length | TX_FIRST_SEGMENT | TX_LAST_SEGMENT;
    out[..4].copy_from_slice(&command_a.to_le_bytes());
    out[4..8].copy_from_slice(&length.to_le_bytes());
    out[TX_HEADER_LEN..total].copy_from_slice(frame);
    Some(total)
}

pub fn read_rx_frame(bytes: &[u8]) -> Option<&[u8]> {
    let status = u32::from_le_bytes(bytes.get(..RX_HEADER_LEN)?.try_into().ok()?);
    if status & RX_ERROR != 0 {
        return None;
    }
    let received_len = ((status & RX_LENGTH_MASK) >> 16) as usize;
    if received_len <= ETHERNET_FCS_LEN {
        return None;
    }
    let received_end = RX_HEADER_LEN.checked_add(received_len)?;
    if received_end > bytes.len() {
        return None;
    }
    Some(&bytes[RX_HEADER_LEN..received_end - ETHERNET_FCS_LEN])
}
