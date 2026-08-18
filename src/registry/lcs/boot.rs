use crate::registry::{
    BOOT_ROOT_KEY, RawRegistryValue, build_boot_success_grace_from_registry_values,
    build_max_parallel_starts_from_registry_values, build_post_kill_timeout_from_registry_values,
    build_settle_timeout_from_registry_values, build_shutdown_timeout_from_registry_values,
};

use super::error::LcsRegistryReadError;
use super::value::raw_registry_value_from_peios;

pub(super) fn read_lcs_max_parallel_starts() -> Result<Option<u32>, LcsRegistryReadError> {
    let values = read_lcs_boot_values()?;
    build_max_parallel_starts_from_registry_values(&values)
        .map_err(LcsRegistryReadError::DecodeBoot)
}

pub(super) fn read_lcs_boot_success_grace_secs() -> Result<Option<u32>, LcsRegistryReadError> {
    let values = read_lcs_boot_values()?;
    build_boot_success_grace_from_registry_values(&values).map_err(LcsRegistryReadError::DecodeBoot)
}

pub(super) fn read_lcs_shutdown_timeout_secs() -> Result<Option<u32>, LcsRegistryReadError> {
    let values = read_lcs_boot_values()?;
    build_shutdown_timeout_from_registry_values(&values).map_err(LcsRegistryReadError::DecodeBoot)
}

pub(super) fn read_lcs_post_kill_timeout_secs() -> Result<Option<u32>, LcsRegistryReadError> {
    let values = read_lcs_boot_values()?;
    build_post_kill_timeout_from_registry_values(&values).map_err(LcsRegistryReadError::DecodeBoot)
}

pub(super) fn read_lcs_settle_timeout_secs() -> Result<Option<u32>, LcsRegistryReadError> {
    let values = read_lcs_boot_values()?;
    build_settle_timeout_from_registry_values(&values).map_err(LcsRegistryReadError::DecodeBoot)
}

fn read_lcs_boot_values() -> Result<Vec<RawRegistryValue>, LcsRegistryReadError> {
    use peios::registry::{Key, KeyAccess, OpenFlags};

    let key = match Key::open(
        None,
        BOOT_ROOT_KEY,
        KeyAccess::QUERY_VALUE,
        OpenFlags::default(),
    ) {
        Ok(key) => key,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(Vec::new()),
        Err(source) => return Err(LcsRegistryReadError::OpenBoot(source)),
    };
    key.query_values_batch(None)
        .map_err(LcsRegistryReadError::ReadBoot)?
        .into_iter()
        .map(raw_registry_value_from_peios)
        .collect::<Result<Vec<_>, _>>()
        .map_err(LcsRegistryReadError::Name)
}
