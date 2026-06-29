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
