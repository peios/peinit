use crate::registry::{
    RegistryConfigWarning, SUPPORTED_SERVICES_SCHEMA_VERSION,
    build_boot_success_grace_from_registry_values, build_control_security_from_registry_values,
    build_control_socket_limits_from_registry_values,
    build_eventd_log_datagram_bytes_from_registry_values,
    build_global_environment_from_registry_values,
    build_max_log_buffer_per_service_from_registry_values,
    build_max_log_line_length_from_registry_values, build_max_parallel_starts_from_registry_values,
    build_shutdown_timeout_from_registry_values, services_schema_warnings,
};
use crate::service::ServiceEnvironmentVariable;

use super::super::{binary, dword, sz};

#[test]
fn builds_global_environment_from_registry_values() {
    let environment = build_global_environment_from_registry_values(&[
        sz("PATH", "/global/bin"),
        sz("LD_PRELOAD", "/lib/test.so"),
    ])
    .expect("global environment");

    assert_eq!(
        environment,
        vec![
            ServiceEnvironmentVariable {
                name: "PATH".to_string(),
                value: "/global/bin".to_string(),
            },
            ServiceEnvironmentVariable {
                name: "LD_PRELOAD".to_string(),
                value: "/lib/test.so".to_string(),
            },
        ],
    );
}

#[test]
fn builds_eventd_log_datagram_ceiling_from_registry_values() {
    assert_eq!(
        build_eventd_log_datagram_bytes_from_registry_values(&[dword(
            "MaxLogDatagramBytes",
            512 * 1024,
        )]),
        Some(512 * 1024),
    );
    assert_eq!(
        build_eventd_log_datagram_bytes_from_registry_values(&[]),
        None,
    );
}

#[test]
fn builds_optional_boot_config_from_registry_values() {
    let values = [
        dword("MaxParallelStarts", 6),
        dword("BootSuccessGrace", 45),
        dword("ShutdownTimeout", 77),
        dword("MaxLogLineLength", 12_000),
        dword("MaxLogBufferPerService", 128_000),
    ];

    assert_eq!(
        build_max_parallel_starts_from_registry_values(&values).expect("max parallel starts"),
        Some(6),
    );
    assert_eq!(
        build_boot_success_grace_from_registry_values(&values).expect("boot success grace"),
        Some(45),
    );
    assert_eq!(
        build_shutdown_timeout_from_registry_values(&values).expect("shutdown timeout"),
        Some(77),
    );
    assert_eq!(
        build_max_log_line_length_from_registry_values(&values).expect("max log line length"),
        Some(12_000),
    );
    assert_eq!(
        build_max_log_buffer_per_service_from_registry_values(&values)
            .expect("max log buffer per service"),
        Some(128_000),
    );
}

#[test]
fn missing_boot_config_values_are_absent() {
    assert_eq!(
        build_max_parallel_starts_from_registry_values(&[]).expect("max parallel starts"),
        None,
    );
    assert_eq!(
        build_boot_success_grace_from_registry_values(&[]).expect("boot success grace"),
        None,
    );
    assert_eq!(
        build_shutdown_timeout_from_registry_values(&[]).expect("shutdown timeout"),
        None,
    );
    assert_eq!(
        build_max_log_line_length_from_registry_values(&[]).expect("max log line length"),
        None,
    );
    assert_eq!(
        build_max_log_buffer_per_service_from_registry_values(&[])
            .expect("max log buffer per service"),
        None,
    );
}

#[test]
fn builds_control_security_and_limits_from_init_values() {
    let values = [
        binary("ControlSecurity", &[0x01, 0x02, 0x03]),
        dword("MaxControlConnections", 7),
        dword("MaxRequestSize", 4096),
        dword("ConnectionTimeout", 12),
    ];

    assert_eq!(
        build_control_security_from_registry_values(&values).expect("control security"),
        crate::control::system::ControlSecurityDescriptor::RegistryBinary(vec![1, 2, 3]),
    );
    let limits = build_control_socket_limits_from_registry_values(&values).expect("limits");
    assert_eq!(limits.max_connections, 7);
    assert_eq!(limits.max_request_bytes, 4096);
    assert_eq!(limits.connection_timeout_secs, 12);
}

#[test]
fn missing_control_init_values_use_defaults() {
    assert_eq!(
        build_control_security_from_registry_values(&[]).expect("control security"),
        crate::control::system::ControlSecurityDescriptor::Default,
    );
    assert_eq!(
        build_control_socket_limits_from_registry_values(&[]).expect("limits"),
        crate::control::socket::ControlSocketLimits::default(),
    );
}

#[test]
fn newer_services_schema_version_reports_warning() {
    assert_eq!(
        services_schema_warnings(SUPPORTED_SERVICES_SCHEMA_VERSION),
        []
    );
    assert_eq!(
        services_schema_warnings(SUPPORTED_SERVICES_SCHEMA_VERSION + 1),
        vec![RegistryConfigWarning::NewerServicesSchemaVersion {
            observed: SUPPORTED_SERVICES_SCHEMA_VERSION + 1,
            supported: SUPPORTED_SERVICES_SCHEMA_VERSION,
        }],
    );
}
