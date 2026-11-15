//! Incremental UART upload decoder; see scripts/upload-pi2-kernel.py for the
//! matching sender. Wire order: GABIXLOAD, three little-endian u32 header fields
//! (version, payload byte count, CRC32), then exactly that many payload bytes.
//! The receiver tracks framing and CRC; callers own storage and timeout timing.

pub const UPLOAD_MARKER: &[u8; 9] = b"GABIXLOAD";
pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_KERNEL_SIZE: u32 = 16 * 1024 * 1024;

const HEADER_LEN: usize = 12;
const VERSION_BYTES: core::ops::Range<usize> = 0..4;
const SIZE_BYTES: core::ops::Range<usize> = 4..8;
const CRC_BYTES: core::ops::Range<usize> = 8..12;
const CRC32_INITIAL: u32 = u32::MAX;
const CRC32_REFLECTED_POLYNOMIAL: u32 = 0xedb8_8320;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadFailure {
    InvalidHeader,
    CrcMismatch,
    Timeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadEvent {
    Waiting,
    /// A byte ready for storage; the whole payload is still unverified.
    Payload {
        offset: u32,
        byte: u8,
    },
    /// CRC matched. Store this final byte too before using the complete image.
    Complete {
        offset: u32,
        byte: u8,
        size: u32,
    },
    /// Discard any previously stored partial image; the decoder has reset.
    Failed {
        reason: UploadFailure,
    },
}

pub struct UploadReceiver {
    state: State,
    max_size: u32,
}

enum State {
    Marker {
        matched: usize,
    },
    Header {
        bytes: [u8; HEADER_LEN],
        len: usize,
    },
    Payload {
        size: u32,
        expected_crc: u32,
        received: u32,
        crc: u32,
    },
}

impl UploadReceiver {
    pub const fn new() -> Self {
        Self::with_max_size(MAX_KERNEL_SIZE)
    }

    pub const fn with_max_size(max_size: u32) -> Self {
        Self {
            state: State::Marker { matched: 0 },
            max_size,
        }
    }

    /// Consume one received byte. Header/marker bytes yield Waiting; payload
    /// events include offsets so the caller can write incrementally into RAM.
    pub fn push(&mut self, byte: u8) -> UploadEvent {
        match &mut self.state {
            State::Marker { matched } => {
                if byte == UPLOAD_MARKER[*matched] {
                    *matched += 1;
                    if *matched == UPLOAD_MARKER.len() {
                        self.state = State::Header {
                            bytes: [0; HEADER_LEN],
                            len: 0,
                        };
                    }
                } else {
                    *matched = usize::from(byte == UPLOAD_MARKER[0]);
                }
                UploadEvent::Waiting
            }
            State::Header { bytes, len } => {
                bytes[*len] = byte;
                *len += 1;
                if *len != HEADER_LEN {
                    return UploadEvent::Waiting;
                }

                let version = u32::from_le_bytes(bytes[VERSION_BYTES].try_into().unwrap());
                let size = u32::from_le_bytes(bytes[SIZE_BYTES].try_into().unwrap());
                let expected_crc = u32::from_le_bytes(bytes[CRC_BYTES].try_into().unwrap());
                if version != PROTOCOL_VERSION || size == 0 || size > self.max_size {
                    self.reset();
                    return UploadEvent::Failed {
                        reason: UploadFailure::InvalidHeader,
                    };
                }
                self.state = State::Payload {
                    size,
                    expected_crc,
                    received: 0,
                    crc: CRC32_INITIAL,
                };
                UploadEvent::Waiting
            }
            State::Payload {
                size,
                expected_crc,
                received,
                crc,
            } => {
                let offset = *received;
                *crc = crc32_byte(*crc, byte);
                *received += 1;
                if *received != *size {
                    return UploadEvent::Payload { offset, byte };
                }

                // The sender uses binascii.crc32: complement the running state.
                let valid = !*crc == *expected_crc;
                let size = *size;
                self.reset();
                if valid {
                    UploadEvent::Complete { offset, byte, size }
                } else {
                    UploadEvent::Failed {
                        reason: UploadFailure::CrcMismatch,
                    }
                }
            }
        }
    }

    pub fn is_upload_active(&self) -> bool {
        !matches!(self.state, State::Marker { .. })
    }

    pub fn is_receiving_payload(&self) -> bool {
        matches!(self.state, State::Payload { .. })
    }

    /// Called by the transport when its own deadline expires. Partial markers
    /// are idle; only an accepted marker/header/payload starts an active upload.
    pub fn timeout(&mut self) -> UploadEvent {
        if self.is_upload_active() {
            self.reset();
            UploadEvent::Failed {
                reason: UploadFailure::Timeout,
            }
        } else {
            UploadEvent::Waiting
        }
    }

    fn reset(&mut self) {
        self.state = State::Marker { matched: 0 };
    }
}

impl Default for UploadReceiver {
    fn default() -> Self {
        Self::new()
    }
}

fn crc32_byte(mut crc: u32, byte: u8) -> u32 {
    crc ^= byte as u32;
    for _ in 0..8 {
        // A low bit of one selects the polynomial; zero selects no XOR.
        // Wrapping subtraction produces all-one or all-zero mask bits.
        let mask = 0u32.wrapping_sub(crc & 1);
        crc = (crc >> 1) ^ (CRC32_REFLECTED_POLYNOMIAL & mask);
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::{UploadEvent, UploadFailure, UploadReceiver, PROTOCOL_VERSION, UPLOAD_MARKER};
    use alloc::vec;
    use alloc::vec::Vec;

    fn upload_for(payload: &[u8]) -> Vec<u8> {
        let mut crc = 0xffff_ffffu32;
        for &byte in payload {
            crc = super::crc32_byte(crc, byte);
        }
        let mut upload = Vec::new();
        upload.extend_from_slice(UPLOAD_MARKER);
        upload.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        upload.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        upload.extend_from_slice(&(!crc).to_le_bytes());
        upload.extend_from_slice(payload);
        upload
    }

    fn feed(receiver: &mut UploadReceiver, bytes: &[u8]) -> UploadEvent {
        bytes
            .iter()
            .copied()
            .map(|byte| receiver.push(byte))
            .filter(|event| *event != UploadEvent::Waiting)
            .last()
            .unwrap_or(UploadEvent::Waiting)
    }

    #[test]
    fn test_kernel_upload_completes_after_valid_crc() {
        let payload = b"gabix";
        let mut stream = Vec::new();
        stream.extend_from_slice(b"stale bytes");
        stream.extend_from_slice(UPLOAD_MARKER);
        stream.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        stream.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        stream.extend_from_slice(&0xcf18_fa88u32.to_le_bytes());
        stream.extend_from_slice(payload);

        let mut receiver = UploadReceiver::new();
        let events: Vec<_> = stream
            .into_iter()
            .filter_map(|byte| match receiver.push(byte) {
                UploadEvent::Waiting => None,
                event => Some(event),
            })
            .collect();

        assert_eq!(
            events,
            vec![
                UploadEvent::Payload {
                    offset: 0,
                    byte: b'g'
                },
                UploadEvent::Payload {
                    offset: 1,
                    byte: b'a'
                },
                UploadEvent::Payload {
                    offset: 2,
                    byte: b'b'
                },
                UploadEvent::Payload {
                    offset: 3,
                    byte: b'i'
                },
                UploadEvent::Complete {
                    offset: 4,
                    byte: b'x',
                    size: 5,
                },
            ]
        );
    }

    #[test]
    fn test_upload_timeout_allows_complete_retry() {
        let mut receiver = UploadReceiver::new();
        for &byte in UPLOAD_MARKER.iter().chain([1, 0].iter()) {
            receiver.push(byte);
        }

        assert_eq!(
            receiver.timeout(),
            UploadEvent::Failed {
                reason: UploadFailure::Timeout
            }
        );

        let payload = b"gabix";
        let mut stream = Vec::new();
        stream.extend_from_slice(UPLOAD_MARKER);
        stream.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        stream.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        stream.extend_from_slice(&0xcf18_fa88u32.to_le_bytes());
        stream.extend_from_slice(payload);

        let last = stream
            .into_iter()
            .map(|byte| receiver.push(byte))
            .filter(|event| *event != UploadEvent::Waiting)
            .last();
        assert_eq!(
            last,
            Some(UploadEvent::Complete {
                offset: 4,
                byte: b'x',
                size: 5,
            })
        );
    }

    #[test]
    fn test_kernel_upload_rejects_bad_crc() {
        let mut receiver = UploadReceiver::new();
        let mut upload = upload_for(b"gabix");
        upload[17] ^= 1;
        assert_eq!(
            feed(&mut receiver, &upload),
            UploadEvent::Failed {
                reason: UploadFailure::CrcMismatch
            }
        );
        assert!(!receiver.is_upload_active());
    }

    #[test]
    fn test_kernel_upload_rejects_unsupported_version() {
        let mut receiver = UploadReceiver::new();
        let mut upload = upload_for(b"gabix");
        upload[9..13].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(
            feed(&mut receiver, &upload[..21]),
            UploadEvent::Failed {
                reason: UploadFailure::InvalidHeader
            }
        );
    }

    #[test]
    fn test_upload_rejects_payload_larger_than_receiver_limit() {
        let mut receiver = UploadReceiver::with_max_size(4);
        let upload = upload_for(b"gabix");

        assert_eq!(
            feed(&mut receiver, &upload[..21]),
            UploadEvent::Failed {
                reason: UploadFailure::InvalidHeader
            }
        );
    }
}
