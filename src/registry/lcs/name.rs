#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryNameDecodeError {
    InteriorNul,
    InvalidUtf8,
}

pub(super) fn decode_name(bytes: Vec<u8>) -> Result<String, RegistryNameDecodeError> {
    if bytes.contains(&0) {
        return Err(RegistryNameDecodeError::InteriorNul);
    }
    String::from_utf8(bytes).map_err(|_| RegistryNameDecodeError::InvalidUtf8)
}
