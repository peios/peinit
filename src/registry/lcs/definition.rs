//! A service's definition read from its key, and changed there, for svctl.
//! What the values mean and whether peinit would take them is
//! `client::Definition`'s to say; this reads and writes them.
//!
//! A change is written in one registry transaction, so peinit, which reads
//! every definition again when any changes, sees all of it or none of it.
//! Each value about to change is read again inside the transaction first,
//! and if it is not what was read before, nothing is written: someone
//! changed it meanwhile, and the person decides again.

use peios::registry::{CreateFlags, Disposition, Key, KeyAccess, OpenFlags, Transaction, ValueType};

use crate::client::Change;
use crate::registry::{RawRegistryValue, RegistryValueType};

use super::SERVICES_ROOT_KEY;
use super::name::decode_name;
use super::value::raw_registry_value_from_peios;

const EACCES: i32 = 13;
const ENOENT: i32 = 2;

/// Whether a deleted definition has gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deleted {
    Gone,
    /// Another layer of the registry defines it too, and what that layer
    /// wrote is there still.
    StillDefined,
}

fn path(service: &str) -> String {
    format!("{SERVICES_ROOT_KEY}\\{service}")
}

/// Why `what` could not be done, in words.
fn why(error: &peios::Error, what: &str) -> String {
    if error.raw_os_error() == Some(EACCES) { format!("you are not allowed to {what}") } else { format!("it could not {what} ({error})") }
}

fn value_type(value_type: RegistryValueType) -> ValueType {
    ValueType::from_raw(match value_type {
        RegistryValueType::Sz => peios::registry::ValueType::SZ.0,
        RegistryValueType::MultiSz => peios::registry::ValueType::MULTI_SZ.0,
        RegistryValueType::Dword => peios::registry::ValueType::DWORD.0,
        RegistryValueType::Binary => peios::registry::ValueType::BINARY.0,
        RegistryValueType::Other(other) => other,
    })
}

/// The values of `service`'s definition, or `None` where it has none.
pub fn read_service_values(service: &str) -> Result<Option<Vec<RawRegistryValue>>, String> {
    let key = match Key::open(None, &path(service), KeyAccess::QUERY_VALUE, OpenFlags::default()) {
        Ok(key) => key,
        Err(error) if error.raw_os_error() == Some(ENOENT) => return Ok(None),
        Err(error) => return Err(why(&error, "read its definition")),
    };
    let records = key.query_values_batch(None).map_err(|error| why(&error, "read its definition"))?;
    records
        .into_iter()
        .map(|record| raw_registry_value_from_peios(record).map_err(|error| format!("a value's name could not be read ({error:?})")))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// Writes `changes` to `service`'s definition, which was read as `found`,
/// making the key first if `create`.
pub fn write_service_changes(service: &str, found: &[RawRegistryValue], changes: &[Change], create: bool) -> Result<(), String> {
    let txn = Transaction::begin().map_err(|error| why(&error, "change its definition"))?;
    let access = KeyAccess::QUERY_VALUE | KeyAccess::SET_VALUE;
    let key = if create {
        let (key, disposition) =
            Key::create(None, &path(service), access, CreateFlags::empty(), None, Some(&txn)).map_err(|error| why(&error, "create its definition"))?;
        if disposition == Disposition::OpenedExisting {
            return Err("it is defined already".into());
        }
        key
    } else {
        match Key::open(None, &path(service), access, OpenFlags::default()) {
            Ok(key) => key,
            Err(error) if error.raw_os_error() == Some(ENOENT) => return Err("it is not defined".into()),
            Err(error) => return Err(why(&error, "change its definition")),
        }
    };
    for change in changes {
        let name = match change {
            Change::Set(value) => &value.name,
            Change::Unset(name) => name,
        };
        let now = match key.query_value(name.as_bytes(), Some(&txn)) {
            Ok(value) => Some((value.ty, value.data)),
            Err(error) if error.raw_os_error() == Some(ENOENT) => None,
            Err(error) => return Err(why(&error, "read its definition")),
        };
        let then = found.iter().find(|value| value.name.eq_ignore_ascii_case(name)).map(|value| (value_type(value.value_type), value.data.clone()));
        if now != then {
            return Err(format!("{name} has changed since it was read, and nothing was written"));
        }
    }
    for change in changes {
        match change {
            Change::Set(value) => key.set_value(value.name.as_bytes(), value_type(value.value_type), &value.data).in_txn(&txn).call(),
            Change::Unset(name) => key.delete_value(name.as_bytes(), None, Some(&txn)),
        }
        .map_err(|error| why(&error, "change its definition"))?;
    }
    txn.commit().map_err(|error| why(&error, "change its definition"))
}

/// Deletes `service`'s definition and whatever is under it (`TimerState`).
pub fn delete_service_definition(service: &str) -> Result<Deleted, String> {
    let txn = Transaction::begin().map_err(|error| why(&error, "delete its definition"))?;
    let key = match Key::open(None, &path(service), KeyAccess::DELETE | KeyAccess::ENUMERATE_SUB_KEYS, OpenFlags::default()) {
        Ok(key) => key,
        Err(error) if error.raw_os_error() == Some(ENOENT) => return Err("it is not defined".into()),
        Err(error) => return Err(why(&error, "delete its definition")),
    };
    delete_tree(&key, &txn)?;
    txn.commit().map_err(|error| why(&error, "delete its definition"))?;
    match Key::open(None, &path(service), KeyAccess::QUERY_VALUE, OpenFlags::default()) {
        Ok(_) => Ok(Deleted::StillDefined),
        Err(_) => Ok(Deleted::Gone),
    }
}

fn delete_tree(key: &Key, txn: &Transaction) -> Result<(), String> {
    let names = key
        .subkeys(Some(txn))
        .map(|subkey| {
            let subkey = subkey.map_err(|error| why(&error, "read what is under its definition"))?;
            decode_name(subkey.name).map_err(|error| format!("a key's name could not be read ({error:?})"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for name in names {
        let child = Key::open(Some(key), &name, KeyAccess::DELETE | KeyAccess::ENUMERATE_SUB_KEYS, OpenFlags::default())
            .map_err(|error| why(&error, "delete what is under its definition"))?;
        delete_tree(&child, txn)?;
    }
    key.delete_key(None, Some(txn)).map_err(|error| why(&error, "delete its definition"))
}
