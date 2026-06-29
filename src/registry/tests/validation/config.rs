use crate::registry::{
    ServiceRegistryDecodeError, build_control_security_from_registry_values,
    build_control_socket_limits_from_registry_values,
    build_global_environment_from_registry_values, build_max_parallel_starts_from_registry_values,
    build_shutdown_timeout_from_registry_values,
};

use super::super::{RawRegistryValue, RegistryValueType, sz};

#[test]
fn malformed_global_environment_values_are_rejected() {
    let err = build_global_environment_from_registry_values(&[RawRegistryValue {
        name: "PATH".to_string(),
        value_type: RegistryValueType::Dword,
        data: 7_u32.to_le_bytes().to_vec(),
    }])
    .expect_err("invalid global environment value");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::TypeMismatch {
            field: "EnvVars",
            expected: RegistryValueType::Sz,
            actual: RegistryValueType::Dword,
        }
    );
}

#[test]
fn invalid_global_environment_names_are_rejected() {
    let err = build_global_environment_from_registry_values(&[sz("BAD=NAME", "value")])
        .expect_err("invalid global environment name");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::InvalidEnvironmentVariable {
            value: "BAD=NAME=value".to_string(),
        }
    );
}

#[test]
fn malformed_boot_config_dword_is_rejected() {
    let err = build_max_parallel_starts_from_registry_values(&[RawRegistryValue {
        name: "MaxParallelStarts".to_string(),
        value_type: RegistryValueType::Dword,
        data: vec![1, 2],
    }])
    .expect_err("malformed dword");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::MalformedDword {
            field: "MaxParallelStarts",
            actual_len: 2,
        },
    );

    let err = build_shutdown_timeout_from_registry_values(&[RawRegistryValue {
        name: "ShutdownTimeout".to_string(),
        value_type: RegistryValueType::Dword,
        data: vec![1, 2],
    }])
    .expect_err("malformed shutdown timeout dword");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::MalformedDword {
            field: "ShutdownTimeout",
            actual_len: 2,
        },
    );
}

#[test]
fn malformed_control_limit_dword_is_rejected() {
    let err = build_control_socket_limits_from_registry_values(&[RawRegistryValue {
        name: "MaxRequestSize".to_string(),
        value_type: RegistryValueType::Dword,
        data: vec![1, 2],
    }])
    .expect_err("malformed dword");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::MalformedDword {
            field: "MaxRequestSize",
            actual_len: 2,
        },
    );
}

#[test]
fn malformed_control_security_type_is_rejected() {
    let err = build_control_security_from_registry_values(&[RawRegistryValue {
        name: "ControlSecurity".to_string(),
        value_type: RegistryValueType::Sz,
        data: b"not-binary\0".to_vec(),
    }])
    .expect_err("invalid control security");

    assert_eq!(
        err,
        ServiceRegistryDecodeError::TypeMismatch {
            field: "ControlSecurity",
            expected: RegistryValueType::Binary,
            actual: RegistryValueType::Sz,
        }
    );
}
