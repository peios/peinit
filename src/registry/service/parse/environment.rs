use crate::service::ServiceEnvironmentVariable;

use crate::registry::value::ServiceRegistryDecodeError;

pub(in crate::registry::service) fn parse_environment_variables(
    values: Vec<String>,
) -> Result<Vec<ServiceEnvironmentVariable>, ServiceRegistryDecodeError> {
    values
        .into_iter()
        .map(|value| {
            let Some((name, variable_value)) = value.split_once('=') else {
                return Err(ServiceRegistryDecodeError::InvalidEnvironmentVariable { value });
            };
            if name.is_empty() || name.contains('\0') {
                return Err(ServiceRegistryDecodeError::InvalidEnvironmentVariable { value });
            }
            Ok(ServiceEnvironmentVariable {
                name: name.to_string(),
                value: variable_value.to_string(),
            })
        })
        .collect()
}
