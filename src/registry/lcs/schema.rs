use crate::registry::{INIT_ROOT_KEY, SERVICES_ROOT_KEY, SUPPORTED_SERVICES_SCHEMA_VERSION};

use super::error::LcsRegistryReadError;

/// Ensure the base service-registry structure exists. On the first boot of a
/// fresh (unprovisioned) system — e.g. a live image — the Machine hive holds
/// only its root key, so `Machine\System\Services` (the key Phase 2 enumerates
/// for service definitions, and the holder of `SchemaVersion`) is absent.
///
/// Create the `System`, `Services`, and `Init` keys (idempotent: the registry
/// `create` opens an existing key rather than failing) and stamp `SchemaVersion`
/// on `Services` if missing. `Services` and `Init` are the two roots the runtime
/// installs registry watches on, and `Services` is what Phase 2 enumerates — all
/// must exist (a watch/enumerate of an absent key is ENOENT) even when empty.
/// `SchemaVersion` is only written when absent, so a future migration that bumps
/// it is never silently downgraded on a later boot.
pub(super) fn provision_lcs_base_registry() -> Result<(), LcsRegistryReadError> {
    use peios::registry::{CreateFlags, Key, KeyAccess, ValueType};

    // Parent of SERVICES_ROOT_KEY. The registry `create` does not materialise
    // intermediate keys, so the `System` key must exist before `Services`.
    const SYSTEM_ROOT_KEY: &str = r"Machine\System";

    let access = KeyAccess::CREATE_SUB_KEY | KeyAccess::QUERY_VALUE | KeyAccess::SET_VALUE;
    let provision_err = |stage: &'static str| {
        move |source: peios::Error| LcsRegistryReadError::Provision { stage, source }
    };

    Key::create(
        None,
        SYSTEM_ROOT_KEY,
        access,
        CreateFlags::empty(),
        None,
        None,
    )
    .map_err(provision_err("create System"))?;
    let (services, _disp) = Key::create(
        None,
        SERVICES_ROOT_KEY,
        access,
        CreateFlags::empty(),
        None,
        None,
    )
    .map_err(provision_err("create Services"))?;
    Key::create(
        None,
        INIT_ROOT_KEY,
        access,
        CreateFlags::empty(),
        None,
        None,
    )
    .map_err(provision_err("create Init"))?;

    match services.query_value(b"SchemaVersion", None) {
        Ok(_) => {}
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
            services
                .set_value(
                    b"SchemaVersion",
                    ValueType::DWORD,
                    &SUPPORTED_SERVICES_SCHEMA_VERSION.to_le_bytes(),
                )
                .call()
                .map_err(provision_err("set SchemaVersion"))?;
        }
        Err(error) => {
            return Err(LcsRegistryReadError::Provision {
                stage: "query SchemaVersion",
                source: error,
            });
        }
    }
    Ok(())
}

pub(super) fn read_lcs_services_schema_version() -> Result<u32, LcsRegistryReadError> {
    use peios::registry::{Key, KeyAccess, OpenFlags, ValueType};

    // A fresh registry (first boot of a live/unprovisioned system) has no
    // Machine\System\Services key, and a partially-populated one may lack the
    // SchemaVersion value. Treat either absence as schema version 0 — there are
    // simply no services configured yet — consistent with every other Phase-2
    // registry reader (boot/init/service/timer all map ENOENT to empty). Only a
    // non-ENOENT failure is fatal.
    let key = match Key::open(
        None,
        SERVICES_ROOT_KEY,
        KeyAccess::QUERY_VALUE,
        OpenFlags::default(),
    ) {
        Ok(key) => key,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(0),
        Err(error) => return Err(LcsRegistryReadError::OpenServicesSchema(error)),
    };
    let value = match key.query_value(b"SchemaVersion", None) {
        Ok(value) => value,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(0),
        Err(error) => return Err(LcsRegistryReadError::ReadServicesSchema(error)),
    };
    if value.ty != ValueType::DWORD {
        return Err(LcsRegistryReadError::InvalidServicesSchemaType(value.ty.0));
    }
    let bytes: [u8; 4] = value.data.as_slice().try_into().map_err(|_| {
        LcsRegistryReadError::InvalidServicesSchemaLength {
            actual_len: value.data.len(),
        }
    })?;
    Ok(u32::from_le_bytes(bytes))
}
