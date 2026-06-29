use super::allocator::IdAllocationError;

const NANOS_PER_MILLI: u64 = 1_000_000;
const UUIDV7_TIMESTAMP_MAX_MS: u64 = 0x0000_ffff_ffff_ffff;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) struct UuidV7([u8; 16]);

impl UuidV7 {
    pub(super) fn from_seed(observed_at_ns: u64, sequence: u64) -> Result<Self, IdAllocationError> {
        let timestamp_ms = observed_at_ns / NANOS_PER_MILLI;
        if timestamp_ms > UUIDV7_TIMESTAMP_MAX_MS {
            return Err(IdAllocationError::TimestampOverflow { observed_at_ns });
        }

        let timestamp = timestamp_ms.to_be_bytes();
        let seq = sequence.to_be_bytes();
        Ok(Self([
            timestamp[2],
            timestamp[3],
            timestamp[4],
            timestamp[5],
            timestamp[6],
            timestamp[7],
            0x70 | (seq[0] & 0x0f),
            seq[1],
            0x80 | (seq[2] & 0x3f),
            seq[3],
            seq[4],
            seq[5],
            seq[6],
            seq[7],
            ((observed_at_ns >> 8) & 0xff) as u8,
            (observed_at_ns & 0xff) as u8,
        ]))
    }

    pub(super) fn as_bytes(self) -> [u8; 16] {
        self.0
    }

    pub(super) fn from_canonical_bytes(bytes: [u8; 16]) -> Result<Self, UuidV7ParseError> {
        if bytes[6] >> 4 != 0x7 {
            return Err(UuidV7ParseError::Version {
                actual: bytes[6] >> 4,
            });
        }
        if bytes[8] >> 6 != 0b10 {
            return Err(UuidV7ParseError::Variant {
                actual: bytes[8] >> 6,
            });
        }
        Ok(Self(bytes))
    }

    pub(super) fn to_canonical_string(self) -> String {
        let b = self.0;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5],
            b[6],
            b[7],
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15],
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UuidV7ParseError {
    Length { actual: usize },
    Hyphen { index: usize },
    Hex { index: usize },
    Version { actual: u8 },
    Variant { actual: u8 },
}

pub(super) fn parse_uuid_v7_canonical(value: &str) -> Result<UuidV7, UuidV7ParseError> {
    if value.len() != 36 {
        return Err(UuidV7ParseError::Length {
            actual: value.len(),
        });
    }

    for index in [8, 13, 18, 23] {
        if value.as_bytes()[index] != b'-' {
            return Err(UuidV7ParseError::Hyphen { index });
        }
    }

    let mut bytes = [0_u8; 16];
    let mut byte_index = 0;
    let mut high_nibble = None;
    for (index, byte) in value.bytes().enumerate() {
        if byte == b'-' {
            continue;
        }
        let nibble = hex_nibble(byte).ok_or(UuidV7ParseError::Hex { index })?;
        if let Some(high) = high_nibble.take() {
            bytes[byte_index] = (high << 4) | nibble;
            byte_index += 1;
        } else {
            high_nibble = Some(nibble);
        }
    }

    UuidV7::from_canonical_bytes(bytes)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
