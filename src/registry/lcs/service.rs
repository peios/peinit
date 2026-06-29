use crate::registry::{
    SERVICES_ROOT_KEY, apply_inherited_service_security,
    build_service_definition_from_registry_values, build_service_security_from_registry_value,
};
use crate::service::{ServiceDefinition, ServiceSecurityDescriptor};

use super::error::LcsRegistryReadError;
use super::name::decode_name;
use super::value::{raw_named_registry_value_from_peios, raw_registry_value_from_peios};

pub(super) fn read_lcs_service_definitions() -> Result<Vec<ServiceDefinition>, LcsRegistryReadError>
{
    use peios::registry::{Key, KeyAccess, OpenFlags};

    let root = Key::open(
        None,
        SERVICES_ROOT_KEY,
        KeyAccess::ENUMERATE_SUB_KEYS | KeyAccess::QUERY_VALUE,
        OpenFlags::default(),
    )
    .map_err(LcsRegistryReadError::OpenRoot)?;
    let inherited_security = read_lcs_inherited_service_security(&root)?;

    let mut names = root
        .subkeys(None)
        .map(|entry| {
            entry
                .map_err(LcsRegistryReadError::EnumerateService)
                .and_then(|subkey| decode_name(subkey.name).map_err(LcsRegistryReadError::Name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();

    let mut definitions = Vec::new();
    for name in names {
        definitions.push(read_lcs_service_definition(&name)?);
    }
    apply_inherited_service_security(&mut definitions, inherited_security);

    Ok(definitions)
}

fn read_lcs_inherited_service_security(
    root: &peios::registry::Key,
) -> Result<Option<ServiceSecurityDescriptor>, LcsRegistryReadError> {
    let value = match root.query_value(b"ServiceSecurity", None) {
        Ok(value) => value,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(None),
        Err(source) => return Err(LcsRegistryReadError::ReadServicesRoot(source)),
    };
    let raw = raw_named_registry_value_from_peios("ServiceSecurity", value);
    build_service_security_from_registry_value(&raw)
        .map(Some)
        .map_err(LcsRegistryReadError::DecodeServicesRoot)
}

fn read_lcs_service_definition(name: &str) -> Result<ServiceDefinition, LcsRegistryReadError> {
    use peios::registry::{Key, KeyAccess, OpenFlags};

    let key_path = format!("{SERVICES_ROOT_KEY}\\{name}");
    let key = Key::open(
        None,
        &key_path,
        KeyAccess::QUERY_VALUE,
        OpenFlags::default(),
    )
    .map_err(|source| LcsRegistryReadError::OpenService {
        service: name.to_string(),
        source,
    })?;
    let values = key
        .query_values_batch(None)
        .map_err(|source| LcsRegistryReadError::ReadValues {
            service: name.to_string(),
            source,
        })?
        .into_iter()
        .map(raw_registry_value_from_peios)
        .collect::<Result<Vec<_>, _>>()
        .map_err(LcsRegistryReadError::Name)?;

    build_service_definition_from_registry_values(name, &values).map_err(|source| {
        LcsRegistryReadError::DecodeService {
            service: name.to_string(),
            source,
        }
    })
}
