use crate::execution::command::ExecutableCommandParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryValueType {
    Sz,
    MultiSz,
    Dword,
    Binary,
    Other(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRegistryValue {
    pub name: String,
    pub value_type: RegistryValueType,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceRegistryDecodeError {
    InvalidServiceName {
        service: String,
    },
    MissingImagePath,
    DuplicateField {
        field: &'static str,
    },
    TypeMismatch {
        field: &'static str,
        expected: RegistryValueType,
        actual: RegistryValueType,
    },
    MalformedString {
        field: &'static str,
        reason: RegistryStringDecodeError,
    },
    MalformedMultiString {
        field: &'static str,
        reason: RegistryMultiStringDecodeError,
    },
    MalformedDword {
        field: &'static str,
        actual_len: usize,
    },
    UnknownDword {
        field: &'static str,
        value: u32,
    },
    InvalidSuccessExitCode {
        value: String,
    },
    InvalidEnvironmentVariable {
        value: String,
    },
    InvalidListEntry {
        field: &'static str,
        value: String,
    },
    InvalidServiceReference {
        field: &'static str,
        value: String,
    },
    InvalidTrigger {
        value: String,
    },
    InvalidAbsolutePath {
        field: &'static str,
        value: String,
    },
    InvalidExecutableCommand {
        field: &'static str,
        value: String,
        source: ExecutableCommandParseError,
    },
    InvalidReloadSignal {
        value: String,
    },
    InvalidCheck {
        field: &'static str,
        value: String,
    },
    NonCachedRegistryCheck {
        field: &'static str,
        key: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryStringDecodeError {
    MissingTerminator,
    InteriorNul { byte_index: usize },
    InvalidUtf8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryMultiStringDecodeError {
    MissingTerminator,
    InvalidUtf8 { item_index: usize },
}

pub(super) fn decode_sz_field(
    value: &RawRegistryValue,
    field: &'static str,
) -> Result<String, ServiceRegistryDecodeError> {
    expect_type(value, field, RegistryValueType::Sz)?;
    decode_registry_string(&value.data)
        .map_err(|reason| ServiceRegistryDecodeError::MalformedString { field, reason })
}

pub(super) fn decode_multi_sz_field(
    value: &RawRegistryValue,
    field: &'static str,
) -> Result<Vec<String>, ServiceRegistryDecodeError> {
    expect_type(value, field, RegistryValueType::MultiSz)?;
    decode_registry_multi_string(&value.data)
        .map_err(|reason| ServiceRegistryDecodeError::MalformedMultiString { field, reason })
}

pub(super) fn decode_dword_field(
    value: &RawRegistryValue,
    field: &'static str,
) -> Result<u32, ServiceRegistryDecodeError> {
    expect_type(value, field, RegistryValueType::Dword)?;
    let bytes: [u8; 4] = value.data.as_slice().try_into().map_err(|_| {
        ServiceRegistryDecodeError::MalformedDword {
            field,
            actual_len: value.data.len(),
        }
    })?;
    Ok(u32::from_le_bytes(bytes))
}

pub(super) fn decode_binary_field(
    value: &RawRegistryValue,
    field: &'static str,
) -> Result<Vec<u8>, ServiceRegistryDecodeError> {
    expect_type(value, field, RegistryValueType::Binary)?;
    Ok(value.data.clone())
}

fn expect_type(
    value: &RawRegistryValue,
    field: &'static str,
    expected: RegistryValueType,
) -> Result<(), ServiceRegistryDecodeError> {
    if value.value_type == expected {
        Ok(())
    } else {
        Err(ServiceRegistryDecodeError::TypeMismatch {
            field,
            expected,
            actual: value.value_type,
        })
    }
}

fn decode_registry_string(data: &[u8]) -> Result<String, RegistryStringDecodeError> {
    if data.last() != Some(&0) {
        return Err(RegistryStringDecodeError::MissingTerminator);
    }
    let payload = &data[..data.len() - 1];
    if let Some(byte_index) = payload.iter().position(|byte| *byte == 0) {
        return Err(RegistryStringDecodeError::InteriorNul { byte_index });
    }
    String::from_utf8(payload.to_vec()).map_err(|_| RegistryStringDecodeError::InvalidUtf8)
}

fn decode_registry_multi_string(
    data: &[u8],
) -> Result<Vec<String>, RegistryMultiStringDecodeError> {
    if data.last() != Some(&0) {
        return Err(RegistryMultiStringDecodeError::MissingTerminator);
    }

    let mut values = Vec::new();
    let mut start = 0;
    let mut item_index = 0;
    while start < data.len() {
        let Some(relative_end) = data[start..].iter().position(|byte| *byte == 0) else {
            return Err(RegistryMultiStringDecodeError::MissingTerminator);
        };
        let end = start + relative_end;
        if end == start && end == data.len() - 1 {
            return Ok(values);
        }
        values.push(
            String::from_utf8(data[start..end].to_vec())
                .map_err(|_| RegistryMultiStringDecodeError::InvalidUtf8 { item_index })?,
        );
        item_index += 1;
        start = end + 1;
    }

    Err(RegistryMultiStringDecodeError::MissingTerminator)
}
