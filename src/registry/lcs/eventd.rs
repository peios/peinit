use crate::registry::{
    EVENTD_ROOT_KEY, RawRegistryValue, build_eventd_log_datagram_bytes_from_registry_values,
    build_eventd_log_socket_path_from_registry_values,
};

use super::error::LcsRegistryReadError;
use super::value::raw_registry_value_from_peios;

pub(super) fn read_lcs_eventd_log_socket_path() -> Result<Option<String>, LcsRegistryReadError> {
    Ok(build_eventd_log_socket_path_from_registry_values(
        &read_lcs_eventd_values()?,
    ))
}

pub(super) fn read_lcs_eventd_log_datagram_bytes() -> Result<Option<u32>, LcsRegistryReadError> {
    Ok(build_eventd_log_datagram_bytes_from_registry_values(
        &read_lcs_eventd_values()?,
    ))
}

fn read_lcs_eventd_values() -> Result<Vec<RawRegistryValue>, LcsRegistryReadError> {
    use peios::registry::{Key, KeyAccess, OpenFlags};

    let key = match Key::open(
        None,
        EVENTD_ROOT_KEY,
        KeyAccess::QUERY_VALUE,
        OpenFlags::default(),
    ) {
        Ok(key) => key,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(Vec::new()),
        Err(source) => return Err(LcsRegistryReadError::OpenEventd(source)),
    };
    let values = key
        .query_values_batch(None)
        .map_err(LcsRegistryReadError::ReadEventd)?
        .into_iter()
        .map(raw_registry_value_from_peios)
        .collect::<Result<Vec<_>, _>>()
        .map_err(LcsRegistryReadError::Name)?;

    Ok(values)
}
