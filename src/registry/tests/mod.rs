mod success;
mod validation;

use super::{RawRegistryValue, RegistryValueType};

fn sz(name: &str, value: &str) -> RawRegistryValue {
    let mut data = value.as_bytes().to_vec();
    data.push(0);
    RawRegistryValue {
        name: name.to_string(),
        value_type: RegistryValueType::Sz,
        data,
    }
}

fn multi_sz(name: &str, values: &[&str]) -> RawRegistryValue {
    let mut data = Vec::new();
    for value in values {
        data.extend_from_slice(value.as_bytes());
        data.push(0);
    }
    data.push(0);
    RawRegistryValue {
        name: name.to_string(),
        value_type: RegistryValueType::MultiSz,
        data,
    }
}

fn dword(name: &str, value: u32) -> RawRegistryValue {
    RawRegistryValue {
        name: name.to_string(),
        value_type: RegistryValueType::Dword,
        data: value.to_le_bytes().to_vec(),
    }
}

fn binary(name: &str, data: &[u8]) -> RawRegistryValue {
    RawRegistryValue {
        name: name.to_string(),
        value_type: RegistryValueType::Binary,
        data: data.to_vec(),
    }
}
