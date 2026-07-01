use crate::provisioning::ServiceRuntimeDirectory;
use crate::registry::fields::Field;
use crate::registry::value::{
    RawRegistryValue, ServiceRegistryDecodeError, decode_multi_sz_field, decode_sz_field,
};
use crate::security::canonical_well_known_identity;

pub(in crate::registry::service) fn parse_non_empty_list(
    value: &RawRegistryValue,
    field: Field,
) -> Result<Vec<String>, ServiceRegistryDecodeError> {
    decode_multi_sz_field(value, field.name())?
        .into_iter()
        .map(|entry| {
            if entry.is_empty() {
                Err(ServiceRegistryDecodeError::InvalidListEntry {
                    field: field.name(),
                    value: entry,
                })
            } else {
                Ok(entry)
            }
        })
        .collect()
}

pub(in crate::registry::service) fn parse_optional_string(
    value: &RawRegistryValue,
    field: Field,
) -> Result<Option<String>, ServiceRegistryDecodeError> {
    let parsed = decode_sz_field(value, field.name())?;
    Ok((!parsed.is_empty()).then_some(parsed))
}

pub(in crate::registry::service) fn parse_identity_field(
    value: &RawRegistryValue,
    field: Field,
) -> Result<Option<String>, ServiceRegistryDecodeError> {
    let parsed = decode_sz_field(value, field.name())?;
    if parsed.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        canonical_well_known_identity(&parsed)
            .unwrap_or(parsed.as_str())
            .to_string(),
    ))
}

pub(in crate::registry::service) fn parse_absolute_path_field(
    value: &RawRegistryValue,
    field: Field,
) -> Result<String, ServiceRegistryDecodeError> {
    let parsed = decode_sz_field(value, field.name())?;
    if parsed.starts_with('/') {
        Ok(parsed)
    } else {
        Err(ServiceRegistryDecodeError::InvalidAbsolutePath {
            field: field.name(),
            value: parsed,
        })
    }
}

pub(in crate::registry::service) fn parse_runtime_directories(
    value: &RawRegistryValue,
    field: Field,
) -> Result<Vec<ServiceRuntimeDirectory>, ServiceRegistryDecodeError> {
    decode_multi_sz_field(value, field.name())?
        .into_iter()
        .map(|entry| {
            if is_valid_runtime_directory_name(&entry) {
                Ok(ServiceRuntimeDirectory { name: entry })
            } else {
                Err(ServiceRegistryDecodeError::InvalidRuntimeDirectory {
                    field: field.name(),
                    value: entry,
                })
            }
        })
        .collect()
}

fn is_valid_runtime_directory_name(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | 0..=31 | 127))
}

#[cfg(test)]
mod tests {
    use super::is_valid_runtime_directory_name;

    #[test]
    fn runtime_directory_names_are_single_relative_components() {
        for valid in ["app", "app.sock.d", "app-cache_1"] {
            assert!(is_valid_runtime_directory_name(valid), "{valid}");
        }
        for invalid in [
            "",
            ".",
            "..",
            "/app",
            "app/cache",
            "app\\cache",
            "bad\nname",
        ] {
            assert!(!is_valid_runtime_directory_name(invalid), "{invalid:?}");
        }
    }
}
