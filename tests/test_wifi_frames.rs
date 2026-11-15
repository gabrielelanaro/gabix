#[path = "../crates/kernel-core/src/net/wifi.rs"]
mod wifi;

#[test]
fn test_beacon_rejects_truncated_information() {
    let mut frame = vec![0u8; 36];
    frame[0] = 0x80;
    frame[16..22].copy_from_slice(&[2, 3, 4, 5, 6, 7]);
    frame.extend_from_slice(&[0, 4, b't', b'e', b's', b't', 3, 1, 6, 48, 2, 1, 0]);
    let beacon = wifi::parse_beacon(&frame).unwrap();
    assert_eq!(beacon.ssid, b"test");
    assert_eq!(beacon.channel, Some(6));
    assert_eq!(beacon.rsn, Some(&[1, 0][..]));
    frame.pop();
    assert!(wifi::parse_beacon(&frame).is_none());
    frame.truncate(35);
    assert!(wifi::parse_beacon(&frame).is_none());
}

#[test]
fn test_ccmp_receive_rejects_replayed_packet_number() {
    let station = [2, 0, 0, 0, 0, 1];
    let ap = [2, 0, 0, 0, 0, 2];
    let sender = [2, 0, 0, 0, 0, 3];
    let mut link = wifi::CcmpLink::new(station, ap, 1, [0; 8]).unwrap();
    // A hardware-verified frame: 24-byte header, CCMP header, SNAP + payload,
    // and the MIC retained by the adapter. Crypto verification precedes this.
    let mut frame = [0; 51];
    frame[..2].copy_from_slice(&[0x08, 0x42]);
    frame[4..10].copy_from_slice(&station);
    frame[10..16].copy_from_slice(&ap);
    frame[16..22].copy_from_slice(&sender);
    frame[24..32].copy_from_slice(&[1, 0, 0, 0x20, 0, 0, 0, 0]);
    frame[32..43].copy_from_slice(&[0xaa, 0xaa, 3, 0, 0, 0, 8, 0, 1, 2, 3]);
    let mut ethernet = [0; 64];
    assert_eq!(link.receive(&frame, &mut ethernet), Some(17));
    assert_eq!(&ethernet[..6], &station);
    assert_eq!(&ethernet[6..12], &sender);
    assert_eq!(&ethernet[12..17], &[8, 0, 1, 2, 3]);
    assert_eq!(link.receive(&frame, &mut ethernet), None);
    frame[24] = 2;
    assert_eq!(link.receive(&frame, &mut ethernet), Some(17));
    frame[10] ^= 2;
    frame[24] = 3;
    assert_eq!(link.receive(&frame, &mut ethernet), None);
    frame[10] ^= 2;
    let mut qos = frame.to_vec();
    qos[0] = 0x88;
    qos.splice(24..24, [0x80, 0]);
    // Aggregated MSDUs require another parser. Rejection must not consume PN3.
    assert_eq!(link.receive(&qos, &mut ethernet), None);
    qos[24] = 0;
    assert_eq!(link.receive(&qos, &mut ethernet), Some(17));
}
