use crate::service::{ServiceCheck, ServiceCheckKind};

use crate::registry::fields::Field;
use crate::registry::value::{RawRegistryValue, ServiceRegistryDecodeError, decode_multi_sz_field};

pub(in crate::registry::service) fn parse_service_checks(
    value: &RawRegistryValue,
    field: Field,
) -> Result<Vec<ServiceCheck>, ServiceRegistryDecodeError> {
    decode_multi_sz_field(value, field.name())?
        .into_iter()
        .map(|entry| parse_service_check(field, entry))
        .collect()
}

fn parse_service_check(
    field: Field,
    value: String,
) -> Result<ServiceCheck, ServiceRegistryDecodeError> {
    let Some((kind, argument)) = value.split_once(':') else {
        return Err(ServiceRegistryDecodeError::InvalidCheck {
            field: field.name(),
            value,
        });
    };
    if argument.is_empty() {
        return Err(ServiceRegistryDecodeError::InvalidCheck {
            field: field.name(),
            value,
        });
    }
    let kind = match kind {
        "path" => ServiceCheckKind::Path,
        "file" => ServiceCheckKind::File,
        "directory" => ServiceCheckKind::Directory,
        "registry" => {
            validate_cached_registry_check(field, argument)?;
            ServiceCheckKind::Registry
        }
        _ => {
            return Err(ServiceRegistryDecodeError::InvalidCheck {
                field: field.name(),
                value,
            });
        }
    };
    Ok(ServiceCheck {
        kind,
        argument: argument.to_string(),
    })
}

fn validate_cached_registry_check(
    field: Field,
    key: &str,
) -> Result<(), ServiceRegistryDecodeError> {
    if key == "Machine\\System\\Services"
        || key.starts_with("Machine\\System\\Services\\")
        || key == "Machine\\System\\Init"
        || key.starts_with("Machine\\System\\Init\\")
    {
        Ok(())
    } else {
        Err(ServiceRegistryDecodeError::NonCachedRegistryCheck {
            field: field.name(),
            key: key.to_string(),
        })
    }
}
