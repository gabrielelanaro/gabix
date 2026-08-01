use gabix_kernel::net::smsc95xx::{is_lan9512, read_rx_frame, write_tx_frame};

#[test]
fn test_lan9512_ids_select_driver() {
    assert!(is_lan9512(0x0424, 0xec00));
    assert!(!is_lan9512(0x0525, 0xa4a2));
}

#[test]
fn test_lan9512_frames_roundtrip() {
    let frame = [0x5au8; 60];
    let mut tx = [0u8; 68];
    let tx_len = write_tx_frame(&frame, &mut tx).unwrap();

    assert_eq!(tx_len, 68);
    assert_eq!(u32::from_le_bytes(tx[0..4].try_into().unwrap()), 0x303c);
    assert_eq!(u32::from_le_bytes(tx[4..8].try_into().unwrap()), 60);
    assert_eq!(&tx[8..], &frame);

    let mut rx = [0u8; 68];
    rx[..4].copy_from_slice(&(64u32 << 16).to_le_bytes());
    rx[4..64].copy_from_slice(&frame);
    assert_eq!(read_rx_frame(&rx), Some(frame.as_slice()));
}

#[test]
fn test_lan9512_errors_drop_frame() {
    let mut rx = [0u8; 68];
    rx[..4].copy_from_slice(&((64u32 << 16) | 0x8000).to_le_bytes());

    assert_eq!(read_rx_frame(&rx), None);
}
