//! Internet checksum helpers used by IPv4, ICMP, and UDP.

pub fn internet_checksum(data: &[u8]) -> u16 {
    internet_checksum_parts(&[data])
}

pub fn internet_checksum_parts(parts: &[&[u8]]) -> u16 {
    let mut sum = 0u32;
    let mut pending = None;

    for part in parts {
        let mut i = 0;
        if let Some(high) = pending.take() {
            if let Some(&low) = part.first() {
                sum += u16::from_be_bytes([high, low]) as u32;
                sum = fold_carry_once(sum);
                i = 1;
            } else {
                pending = Some(high);
            }
        }
        while i + 1 < part.len() {
            sum += u16::from_be_bytes([part[i], part[i + 1]]) as u32;
            sum = fold_carry_once(sum);
            i += 2;
        }
        if i < part.len() {
            pending = Some(part[i]);
        }
    }

    if let Some(high) = pending {
        sum += (high as u32) << 8;
    }
    while sum > 0xffff {
        sum = fold_carry_once(sum);
    }
    !(sum as u16)
}

fn fold_carry_once(sum: u32) -> u32 {
    (sum & 0xffff) + (sum >> 16)
}
