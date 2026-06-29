use crate::service::is_valid_service_name;

use crate::registry::fields::Field;
use crate::registry::value::{
    RawRegistryValue, ServiceRegistryDecodeError, decode_multi_sz_field, decode_sz_field,
};

pub(in crate::registry::service) fn validate_service_name(value: &str) -> Result<(), ()> {
    if is_valid_service_name(value) {
        Ok(())
    } else {
        Err(())
    }
}

pub(in crate::registry::service) fn parse_service_reference_list(
    value: &RawRegistryValue,
    field: Field,
) -> Result<Vec<String>, ServiceRegistryDecodeError> {
    decode_multi_sz_field(value, field.name())?
        .into_iter()
        .map(|entry| {
            validate_service_reference(field, &entry)?;
            Ok(entry)
        })
        .collect()
}

pub(in crate::registry::service) fn parse_service_reference_field(
    value: &RawRegistryValue,
    field: Field,
) -> Result<String, ServiceRegistryDecodeError> {
    let parsed = decode_sz_field(value, field.name())?;
    validate_service_reference(field, &parsed)?;
    Ok(parsed)
}

fn validate_service_reference(field: Field, value: &str) -> Result<(), ServiceRegistryDecodeError> {
    validate_service_name(value).map_err(|_| ServiceRegistryDecodeError::InvalidServiceReference {
        field: field.name(),
        value: value.to_string(),
    })
}
