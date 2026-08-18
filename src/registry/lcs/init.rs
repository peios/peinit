use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::registry::{
    INIT_ROOT_KEY, RawRegistryValue, build_control_security_from_registry_values,
    build_control_socket_limits_from_registry_values,
    build_log_read_bytes_per_event_from_registry_values,
    build_max_log_buffer_per_service_from_registry_values,
    build_max_log_line_length_from_registry_values, build_pre_eventd_buffer_from_registry_values,
};

use super::error::LcsRegistryReadError;
use super::value::raw_registry_value_from_peios;

pub(super) fn read_lcs_control_security() -> Result<ControlSecurityDescriptor, LcsRegistryReadError>
{
    let values = read_lcs_init_values()?;
    build_control_security_from_registry_values(&values).map_err(LcsRegistryReadError::DecodeInit)
}

pub(super) fn read_lcs_control_socket_limits() -> Result<ControlSocketLimits, LcsRegistryReadError>
{
    let values = read_lcs_init_values()?;
    build_control_socket_limits_from_registry_values(&values)
        .map_err(LcsRegistryReadError::DecodeInit)
}

pub(super) fn read_lcs_max_log_line_length() -> Result<Option<u32>, LcsRegistryReadError> {
    let values = read_lcs_init_values()?;
    build_max_log_line_length_from_registry_values(&values)
        .map_err(LcsRegistryReadError::DecodeInit)
}

pub(super) fn read_lcs_max_log_buffer_per_service() -> Result<Option<u32>, LcsRegistryReadError> {
    let values = read_lcs_init_values()?;
    build_max_log_buffer_per_service_from_registry_values(&values)
        .map_err(LcsRegistryReadError::DecodeInit)
}

pub(super) fn read_lcs_log_read_bytes_per_event() -> Result<Option<u32>, LcsRegistryReadError> {
    let values = read_lcs_init_values()?;
    build_log_read_bytes_per_event_from_registry_values(&values)
        .map_err(LcsRegistryReadError::DecodeInit)
}

pub(super) fn read_lcs_pre_eventd_buffer_bytes() -> Result<Option<u32>, LcsRegistryReadError> {
    let values = read_lcs_init_values()?;
    build_pre_eventd_buffer_from_registry_values(&values).map_err(LcsRegistryReadError::DecodeInit)
}

fn read_lcs_init_values() -> Result<Vec<RawRegistryValue>, LcsRegistryReadError> {
    use peios::registry::{Key, KeyAccess, OpenFlags};

    let key = match Key::open(
        None,
        INIT_ROOT_KEY,
        KeyAccess::QUERY_VALUE,
        OpenFlags::default(),
    ) {
        Ok(key) => key,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(Vec::new()),
        Err(source) => return Err(LcsRegistryReadError::OpenInit(source)),
    };
    key.query_values_batch(None)
        .map_err(LcsRegistryReadError::ReadInit)?
        .into_iter()
        .map(raw_registry_value_from_peios)
        .collect::<Result<Vec<_>, _>>()
        .map_err(LcsRegistryReadError::Name)
}
