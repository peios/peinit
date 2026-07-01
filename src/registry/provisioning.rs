use std::collections::BTreeSet;

use crate::provisioning::{
    ProvisionedPath, ProvisionedPathKind, ProvisionedPathRegistrySnapshot,
    ProvisionedPathRegistryWarning, ProvisionedPathSecurity,
};
use crate::service::is_valid_service_name;

use super::fields::parse_bool;
use super::value::{
    RawRegistryValue, ServiceRegistryDecodeError, decode_binary_field, decode_dword_field,
    decode_sz_field,
};

const KIND_FIELD: &str = "Kind";
const PATH_FIELD: &str = "Path";
const SECURITY_FIELD: &str = "Security";
const REQUIRED_FIELD: &str = "Required";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProvisionedPathField {
    Kind,
    Path,
    Security,
    Required,
}

impl ProvisionedPathField {
    fn from_name(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case(KIND_FIELD) {
            Some(Self::Kind)
        } else if name.eq_ignore_ascii_case(PATH_FIELD) {
            Some(Self::Path)
        } else if name.eq_ignore_ascii_case(SECURITY_FIELD) {
            Some(Self::Security)
        } else if name.eq_ignore_ascii_case(REQUIRED_FIELD) {
            Some(Self::Required)
        } else {
            None
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Kind => KIND_FIELD,
            Self::Path => PATH_FIELD,
            Self::Security => SECURITY_FIELD,
            Self::Required => REQUIRED_FIELD,
        }
    }
}

pub fn build_provisioned_path_registry_snapshot(
    raw_entries: Vec<(String, Vec<RawRegistryValue>)>,
) -> ProvisionedPathRegistrySnapshot {
    let mut snapshot = ProvisionedPathRegistrySnapshot::empty();
    for (name, values) in raw_entries {
        match build_provisioned_path_from_registry_values(&name, &values) {
            Ok(entry) => snapshot.entries.push(entry),
            Err(error) => snapshot.warnings.push(ProvisionedPathRegistryWarning {
                entry: name,
                message: format!("{error:?}"),
            }),
        }
    }
    snapshot
}

pub fn build_provisioned_path_from_registry_values(
    name: &str,
    values: &[RawRegistryValue],
) -> Result<ProvisionedPath, ServiceRegistryDecodeError> {
    if !is_valid_service_name(name) {
        return Err(ServiceRegistryDecodeError::InvalidProvisionedPathName {
            name: name.to_string(),
        });
    }

    let mut builder = ProvisionedPathBuilder::new(name);
    let mut seen = BTreeSet::new();
    for value in values {
        let Some(field) = ProvisionedPathField::from_name(&value.name) else {
            continue;
        };
        if !seen.insert(field) {
            return Err(ServiceRegistryDecodeError::DuplicateField {
                field: field.name(),
            });
        }
        builder.apply(field, value)?;
    }
    builder.finish()
}

struct ProvisionedPathBuilder {
    name: String,
    kind: Option<ProvisionedPathKind>,
    path: Option<String>,
    security: ProvisionedPathSecurity,
    required: bool,
}

impl ProvisionedPathBuilder {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            kind: None,
            path: None,
            security: ProvisionedPathSecurity::Default,
            required: false,
        }
    }

    fn apply(
        &mut self,
        field: ProvisionedPathField,
        value: &RawRegistryValue,
    ) -> Result<(), ServiceRegistryDecodeError> {
        match field {
            ProvisionedPathField::Kind => {
                self.kind = Some(parse_kind(&self.name, decode_sz_field(value, KIND_FIELD)?)?);
            }
            ProvisionedPathField::Path => {
                let path = decode_sz_field(value, PATH_FIELD)?;
                if !path.starts_with('/') {
                    return Err(ServiceRegistryDecodeError::InvalidAbsolutePath {
                        field: PATH_FIELD,
                        value: path,
                    });
                }
                self.path = Some(path);
            }
            ProvisionedPathField::Security => {
                self.security = ProvisionedPathSecurity::RegistryBinary(decode_binary_field(
                    value,
                    SECURITY_FIELD,
                )?);
            }
            ProvisionedPathField::Required => {
                self.required =
                    parse_bool(REQUIRED_FIELD, decode_dword_field(value, REQUIRED_FIELD)?)?;
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<ProvisionedPath, ServiceRegistryDecodeError> {
        let kind =
            self.kind
                .ok_or_else(|| ServiceRegistryDecodeError::MissingProvisionedPathField {
                    entry: self.name.clone(),
                    field: KIND_FIELD,
                })?;
        let path =
            self.path
                .ok_or_else(|| ServiceRegistryDecodeError::MissingProvisionedPathField {
                    entry: self.name.clone(),
                    field: PATH_FIELD,
                })?;
        Ok(ProvisionedPath {
            name: self.name,
            kind,
            path,
            security: self.security,
            required: self.required,
        })
    }
}

fn parse_kind(
    entry: &str,
    value: String,
) -> Result<ProvisionedPathKind, ServiceRegistryDecodeError> {
    match value.as_str() {
        "directory" => Ok(ProvisionedPathKind::Directory),
        "file" => Ok(ProvisionedPathKind::File),
        _ => Err(ServiceRegistryDecodeError::InvalidProvisionedPathKind {
            entry: entry.to_string(),
            value,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{RawRegistryValue, RegistryValueType};

    #[test]
    fn builds_provisioned_path_entry() {
        let entry = build_provisioned_path_from_registry_values(
            "eventd",
            &[
                sz("Kind", "directory"),
                sz("Path", "/run/eventd"),
                binary("Security", &[1, 2, 3]),
                dword("Required", 1),
            ],
        )
        .expect("entry");

        assert_eq!(
            entry,
            ProvisionedPath {
                name: "eventd".to_string(),
                kind: ProvisionedPathKind::Directory,
                path: "/run/eventd".to_string(),
                security: ProvisionedPathSecurity::RegistryBinary(vec![1, 2, 3]),
                required: true,
            }
        );
    }

    #[test]
    fn snapshot_skips_invalid_entries_with_warnings() {
        let snapshot = build_provisioned_path_registry_snapshot(vec![
            (
                "valid".to_string(),
                vec![sz("Kind", "file"), sz("Path", "/run/valid")],
            ),
            (
                "broken".to_string(),
                vec![sz("Kind", "unknown"), sz("Path", "/run/broken")],
            ),
        ]);

        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(snapshot.entries[0].name, "valid");
        assert_eq!(snapshot.warnings.len(), 1);
        assert_eq!(snapshot.warnings[0].entry, "broken");
    }

    fn sz(name: &str, value: &str) -> RawRegistryValue {
        let mut data = value.as_bytes().to_vec();
        data.push(0);
        RawRegistryValue {
            name: name.to_string(),
            value_type: RegistryValueType::Sz,
            data,
        }
    }

    fn dword(name: &str, value: u32) -> RawRegistryValue {
        RawRegistryValue {
            name: name.to_string(),
            value_type: RegistryValueType::Dword,
            data: value.to_le_bytes().to_vec(),
        }
    }

    fn binary(name: &str, data: &[u8]) -> RawRegistryValue {
        RawRegistryValue {
            name: name.to_string(),
            value_type: RegistryValueType::Binary,
            data: data.to_vec(),
        }
    }
}
