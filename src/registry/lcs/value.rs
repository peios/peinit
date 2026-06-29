use crate::registry::{RawRegistryValue, RegistryValueType};

use super::name::{RegistryNameDecodeError, decode_name};

pub(super) fn raw_registry_value_from_peios(
    record: peios::registry::ValueRecord,
) -> Result<RawRegistryValue, RegistryNameDecodeError> {
    Ok(RawRegistryValue {
        name: decode_name(record.name)?,
        value_type: registry_value_type_from_peios(record.ty),
        data: record.data,
    })
}

pub(super) fn raw_named_registry_value_from_peios(
    name: &str,
    value: peios::registry::RegValue,
) -> RawRegistryValue {
    RawRegistryValue {
        name: name.to_string(),
        value_type: registry_value_type_from_peios(value.ty),
        data: value.data,
    }
}

fn registry_value_type_from_peios(ty: peios::registry::ValueType) -> RegistryValueType {
    if ty == peios::registry::ValueType::SZ {
        RegistryValueType::Sz
    } else if ty == peios::registry::ValueType::MULTI_SZ {
        RegistryValueType::MultiSz
    } else if ty == peios::registry::ValueType::DWORD {
        RegistryValueType::Dword
    } else if ty == peios::registry::ValueType::BINARY {
        RegistryValueType::Binary
    } else {
        RegistryValueType::Other(ty.0)
    }
}
