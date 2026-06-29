use std::fmt;

use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::service::ServiceEnvironmentVariable;

use super::value::{
    RawRegistryValue, ServiceRegistryDecodeError, decode_binary_field, decode_dword_field,
    decode_sz_field,
};

pub const SUPPORTED_SERVICES_SCHEMA_VERSION: u32 = 1;

const LOG_SOCKET_PATH_FIELD: &str = "LogSocketPath";
const MAX_PARALLEL_STARTS_FIELD: &str = "MaxParallelStarts";
const BOOT_SUCCESS_GRACE_FIELD: &str = "BootSuccessGrace";
const SHUTDOWN_TIMEOUT_FIELD: &str = "ShutdownTimeout";
const MAX_LOG_LINE_LENGTH_FIELD: &str = "MaxLogLineLength";
const MAX_LOG_BUFFER_PER_SERVICE_FIELD: &str = "MaxLogBufferPerService";
const CONTROL_SECURITY_FIELD: &str = "ControlSecurity";
const MAX_CONTROL_CONNECTIONS_FIELD: &str = "MaxControlConnections";
const MAX_REQUEST_SIZE_FIELD: &str = "MaxRequestSize";
const CONNECTION_TIMEOUT_FIELD: &str = "ConnectionTimeout";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryConfigWarning {
    NewerServicesSchemaVersion { observed: u32, supported: u32 },
}

impl fmt::Display for RegistryConfigWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NewerServicesSchemaVersion {
                observed,
                supported,
            } => write!(
                formatter,
                "services schema version {observed} is newer than supported version {supported}; continuing with forward-compatible decoding",
            ),
        }
    }
}

pub fn services_schema_warnings(schema_version: u32) -> Vec<RegistryConfigWarning> {
    if schema_version > SUPPORTED_SERVICES_SCHEMA_VERSION {
        vec![RegistryConfigWarning::NewerServicesSchemaVersion {
            observed: schema_version,
            supported: SUPPORTED_SERVICES_SCHEMA_VERSION,
        }]
    } else {
        Vec::new()
    }
}

pub fn build_global_environment_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Vec<ServiceEnvironmentVariable>, ServiceRegistryDecodeError> {
    values
        .iter()
        .map(|value| {
            let decoded = decode_sz_field(value, "EnvVars")?;
            if value.name.is_empty() || value.name.contains('=') {
                return Err(ServiceRegistryDecodeError::InvalidEnvironmentVariable {
                    value: format!("{}={decoded}", value.name),
                });
            }
            Ok(ServiceEnvironmentVariable {
                name: value.name.clone(),
                value: decoded,
            })
        })
        .collect()
}

pub fn build_eventd_log_socket_path_from_registry_values(
    values: &[RawRegistryValue],
) -> Option<String> {
    values
        .iter()
        .find(|value| value.name == LOG_SOCKET_PATH_FIELD)
        .and_then(|value| decode_sz_field(value, LOG_SOCKET_PATH_FIELD).ok())
        .filter(|path| !path.is_empty())
}

pub fn build_max_parallel_starts_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    build_optional_boot_dword_from_registry_values(values, MAX_PARALLEL_STARTS_FIELD)
}

pub fn build_boot_success_grace_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    build_optional_boot_dword_from_registry_values(values, BOOT_SUCCESS_GRACE_FIELD)
}

pub fn build_shutdown_timeout_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    build_optional_boot_dword_from_registry_values(values, SHUTDOWN_TIMEOUT_FIELD)
}

pub fn build_max_log_line_length_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    build_optional_boot_dword_from_registry_values(values, MAX_LOG_LINE_LENGTH_FIELD)
}

pub fn build_max_log_buffer_per_service_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    build_optional_boot_dword_from_registry_values(values, MAX_LOG_BUFFER_PER_SERVICE_FIELD)
}

pub fn build_control_security_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<ControlSecurityDescriptor, ServiceRegistryDecodeError> {
    values
        .iter()
        .find(|value| value.name == CONTROL_SECURITY_FIELD)
        .map(|value| {
            decode_binary_field(value, CONTROL_SECURITY_FIELD)
                .map(ControlSecurityDescriptor::RegistryBinary)
        })
        .transpose()
        .map(|descriptor| descriptor.unwrap_or(ControlSecurityDescriptor::Default))
}

pub fn build_control_socket_limits_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<ControlSocketLimits, ServiceRegistryDecodeError> {
    let defaults = ControlSocketLimits::default();
    Ok(ControlSocketLimits {
        max_connections: optional_init_dword(values, MAX_CONTROL_CONNECTIONS_FIELD)?
            .map(|value| value as usize)
            .unwrap_or(defaults.max_connections),
        max_request_bytes: optional_init_dword(values, MAX_REQUEST_SIZE_FIELD)?
            .map(|value| value as usize)
            .unwrap_or(defaults.max_request_bytes),
        connection_timeout_secs: optional_init_dword(values, CONNECTION_TIMEOUT_FIELD)?
            .map(u64::from)
            .unwrap_or(defaults.connection_timeout_secs),
    })
}

fn build_optional_boot_dword_from_registry_values(
    values: &[RawRegistryValue],
    field: &'static str,
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    optional_init_dword(values, field)
}

fn optional_init_dword(
    values: &[RawRegistryValue],
    field: &'static str,
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    values
        .iter()
        .find(|value| value.name == field)
        .map(|value| decode_dword_field(value, field))
        .transpose()
}
