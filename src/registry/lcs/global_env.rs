use crate::registry::{GLOBAL_ENV_VARS_KEY, build_global_environment_from_registry_values};
use crate::service::ServiceEnvironmentVariable;

use super::error::LcsRegistryReadError;
use super::value::raw_registry_value_from_peios;

pub(super) fn read_lcs_global_environment()
-> Result<Vec<ServiceEnvironmentVariable>, LcsRegistryReadError> {
    use peios::registry::{Key, KeyAccess, OpenFlags};

    let key = match Key::open(
        None,
        GLOBAL_ENV_VARS_KEY,
        KeyAccess::QUERY_VALUE,
        OpenFlags::default(),
    ) {
        Ok(key) => key,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(Vec::new()),
        Err(source) => return Err(LcsRegistryReadError::OpenGlobalEnvironment(source)),
    };
    let values = key
        .query_values_batch(None)
        .map_err(LcsRegistryReadError::ReadGlobalEnvironment)?
        .into_iter()
        .map(raw_registry_value_from_peios)
        .collect::<Result<Vec<_>, _>>()
        .map_err(LcsRegistryReadError::Name)?;

    build_global_environment_from_registry_values(&values)
        .map_err(LcsRegistryReadError::DecodeGlobalEnvironment)
}
