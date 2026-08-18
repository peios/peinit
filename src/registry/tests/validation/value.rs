use crate::registry::{
    RegistryMultiStringDecodeError, RegistryStringDecodeError, ServiceRegistryDecodeError,
    build_service_definition_from_registry_values,
};

use super::super::{RawRegistryValue, RegistryValueType, sz};

#[test]
fn consumed_fields_reject_type_mismatch() {
    let err = build_service_definition_from_registry_values(
        "broken",
        &[RawRegistryValue {
            name: "ImagePath".to_string(),
            value_type: RegistryValueType::Dword,
            data: 7_u32.to_le_bytes().to_vec(),
        }],
    )
    .expect_err("type mismatch");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::TypeMismatch {
            field: "ImagePath",
            expected: RegistryValueType::Sz,
            actual: RegistryValueType::Dword,
        }
    );
}

#[test]
fn malformed_strings_are_rejected() {
    let err = build_service_definition_from_registry_values(
        "broken",
        &[RawRegistryValue {
            name: "ImagePath".to_string(),
            value_type: RegistryValueType::Sz,
            data: b"/bin/app".to_vec(),
        }],
    )
    .expect_err("malformed string");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::MalformedString {
            field: "ImagePath",
            reason: RegistryStringDecodeError::MissingTerminator,
        }
    );
}

#[test]
fn malformed_multi_strings_are_rejected() {
    let err = build_service_definition_from_registry_values(
        "broken",
        &[
            sz("ImagePath", "/bin/app"),
            RawRegistryValue {
                name: "Triggers".to_string(),
                value_type: RegistryValueType::MultiSz,
                data: b"boot\0timer:daily".to_vec(),
            },
        ],
    )
    .expect_err("malformed multi string");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::MalformedMultiString {
            field: "Triggers",
            reason: RegistryMultiStringDecodeError::MissingTerminator,
        }
    );
}
