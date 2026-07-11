//! Internet checksum helpers used by IPv4, ICMP, and UDP.

/// Compute the RFC 1071 one's-complement checksum over `data`.
///
/// Bytes are read as big-endian 16-bit words. An odd trailing byte is padded
/// with a zero low byte for the sum. The returned value is the inverted folded
/// sum, ready to write into a checksum field.
pub fn internet_checksum(data: &[u8]) -> u16 {
    internet_checksum_parts(&[data])
}

/// Compute the RFC 1071 checksum over adjacent byte slices.
///
/// A pending byte is carried across slice boundaries so a checksum over
/// `[a, b]` has the same result as a checksum over one concatenated buffer.
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
        sum += (high as u16 as u32) << 8;
    }

    while sum > 0xffff {
        sum = fold_carry_once(sum);
    }

    !(sum as u16)
}

fn fold_carry_once(sum: u32) -> u32 {
    (sum & 0xffff) + (sum >> 16)
}
