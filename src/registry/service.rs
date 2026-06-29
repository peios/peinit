use std::collections::BTreeSet;

use crate::service::{ServiceDefinition, ServiceSecurityDescriptor};

mod apply;
mod builder;
mod parse;

use super::fields::field_from_name;
use super::value::{RawRegistryValue, ServiceRegistryDecodeError, decode_binary_field};
use apply::apply_service_field;
use builder::DefinitionBuilder;
use parse::validate_service_name;

pub fn build_service_definition_from_registry_values(
    service_name: &str,
    values: &[RawRegistryValue],
) -> Result<ServiceDefinition, ServiceRegistryDecodeError> {
    validate_service_name(service_name).map_err(|_| {
        ServiceRegistryDecodeError::InvalidServiceName {
            service: service_name.to_string(),
        }
    })?;

    let mut builder = DefinitionBuilder::new(service_name);
    let mut seen_fields = BTreeSet::new();

    for value in values {
        let Some(field) = field_from_name(&value.name) else {
            continue;
        };
        if !seen_fields.insert(field) {
            return Err(ServiceRegistryDecodeError::DuplicateField {
                field: field.name(),
            });
        }

        apply_service_field(&mut builder, field, value)?;
    }

    builder.finish()
}

pub fn build_service_security_from_registry_value(
    value: &RawRegistryValue,
) -> Result<ServiceSecurityDescriptor, ServiceRegistryDecodeError> {
    Ok(ServiceSecurityDescriptor::RegistryBinary(
        decode_binary_field(value, "ServiceSecurity")?,
    ))
}

pub fn apply_inherited_service_security(
    definitions: &mut [ServiceDefinition],
    inherited: Option<ServiceSecurityDescriptor>,
) {
    let Some(inherited) = inherited else {
        return;
    };
    if matches!(inherited, ServiceSecurityDescriptor::Default) {
        return;
    }
    for definition in definitions {
        if matches!(
            definition.service_security,
            ServiceSecurityDescriptor::Default
        ) {
            definition.service_security = inherited.clone();
        }
    }
}
