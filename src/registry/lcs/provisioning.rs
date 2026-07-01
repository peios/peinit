use crate::provisioning::ProvisionedPathRegistrySnapshot;
use crate::registry::{RawRegistryValue, build_provisioned_path_registry_snapshot};

use super::error::LcsRegistryReadError;
use super::name::decode_name;
use super::value::raw_registry_value_from_peios;

const PROVISIONED_PATHS_ROOT_KEY: &str = r"Machine\System\Init\ProvisionedPaths";

pub(super) fn read_lcs_provisioned_paths()
-> Result<ProvisionedPathRegistrySnapshot, LcsRegistryReadError> {
    use peios::registry::{Key, KeyAccess, OpenFlags};

    let root = match Key::open(
        None,
        PROVISIONED_PATHS_ROOT_KEY,
        KeyAccess::ENUMERATE_SUB_KEYS,
        OpenFlags::default(),
    ) {
        Ok(root) => root,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
            return Ok(ProvisionedPathRegistrySnapshot::empty());
        }
        Err(source) => return Err(LcsRegistryReadError::OpenProvisionedPaths(source)),
    };

    let mut raw_entries = Vec::new();
    for entry in root.subkeys(None) {
        let name = entry
            .map_err(LcsRegistryReadError::EnumerateProvisionedPath)
            .and_then(|subkey| decode_name(subkey.name).map_err(LcsRegistryReadError::Name))?;
        raw_entries.push((name.clone(), read_lcs_provisioned_path_values(&name)?));
    }
    raw_entries.sort_by(|left, right| left.0.cmp(&right.0));
    raw_entries.dedup_by(|left, right| left.0 == right.0);

    Ok(build_provisioned_path_registry_snapshot(raw_entries))
}

fn read_lcs_provisioned_path_values(
    name: &str,
) -> Result<Vec<RawRegistryValue>, LcsRegistryReadError> {
    use peios::registry::{Key, KeyAccess, OpenFlags};

    let key_path = format!("{PROVISIONED_PATHS_ROOT_KEY}\\{name}");
    let key = Key::open(
        None,
        &key_path,
        KeyAccess::QUERY_VALUE,
        OpenFlags::default(),
    )
    .map_err(|source| LcsRegistryReadError::OpenProvisionedPath {
        entry: name.to_string(),
        source,
    })?;
    key.query_values_batch(None)
        .map_err(|source| LcsRegistryReadError::ReadProvisionedPath {
            entry: name.to_string(),
            source,
        })?
        .into_iter()
        .map(raw_registry_value_from_peios)
        .collect::<Result<Vec<_>, _>>()
        .map_err(LcsRegistryReadError::Name)
}
