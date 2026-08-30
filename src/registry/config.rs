use std::fmt;

use crate::boundary::{BoundaryError, RegistryClient};
use crate::control::socket::ControlSocketLimits;
use crate::control::system::ControlSecurityDescriptor;
use crate::jobs::socket::JobsSocketLimits;
use crate::logging::{
    DEFAULT_EVENTD_LOG_DATAGRAM_BYTES, DEFAULT_LOG_READ_BYTES_PER_EVENT,
    DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES, DEFAULT_MAX_LOG_LINE_BYTES,
    DEFAULT_PRE_EVENTD_BUFFER_BYTES, RuntimeLogConfig,
};
use crate::service::ServiceEnvironmentVariable;

use super::value::{
    RawRegistryValue, ServiceRegistryDecodeError, decode_binary_field, decode_dword_field,
    decode_sz_field,
};

pub const SUPPORTED_SERVICES_SCHEMA_VERSION: u32 = 1;

const LOG_SOCKET_PATH_FIELD: &str = "LogSocketPath";
const MAX_LOG_DATAGRAM_BYTES_FIELD: &str = "MaxLogDatagramBytes";
const MAX_PARALLEL_STARTS_FIELD: &str = "MaxParallelStarts";
const BOOT_SUCCESS_GRACE_FIELD: &str = "BootSuccessGrace";
const SHUTDOWN_TIMEOUT_FIELD: &str = "ShutdownTimeout";
const POST_KILL_TIMEOUT_FIELD: &str = "PostKillTimeout";
const SETTLE_TIMEOUT_FIELD: &str = "SettleTimeout";
const MAX_LOG_LINE_LENGTH_FIELD: &str = "MaxLogLineLength";
const MAX_LOG_BUFFER_PER_SERVICE_FIELD: &str = "MaxLogBufferPerService";
const LOG_READ_BYTES_PER_EVENT_FIELD: &str = "LogReadBytesPerEvent";
const PRE_EVENTD_BUFFER_FIELD: &str = "PreEventdBuffer";
const CONTROL_SECURITY_FIELD: &str = "ControlSecurity";
const MAX_CONTROL_CONNECTIONS_FIELD: &str = "MaxControlConnections";
const MAX_REQUEST_SIZE_FIELD: &str = "MaxRequestSize";
const CONNECTION_TIMEOUT_FIELD: &str = "ConnectionTimeout";
const MAX_JOBS_CONNECTIONS_FIELD: &str = "MaxJobsConnections";
const MAX_JOB_MESSAGE_SIZE_FIELD: &str = "MaxJobMessageSize";
const JOBS_CONNECTION_TIMEOUT_FIELD: &str = "JobsConnectionTimeout";
const MAX_JOBS_PER_SUBMITTER_FIELD: &str = "MaxJobsPerSubmitter";

/// The smallest useful value for each log tuning knob.
///
/// None of these has a normative floor: the four keys do not appear in the
/// PCSA books at all, and the peinit TRM states defaults without ranges. They
/// are engineering judgement, chosen so that a value below them means the
/// mechanism does not work rather than works differently.
///
/// `MaxLogBufferPerService` is the exception with a real external constraint:
/// it is applied with `F_SETPIPE_SZ`, whose kernel minimum is one page, and
/// the kernel rounds up to a page regardless. Asking for less is not a smaller
/// pipe, it is the same pipe and a misleading number in the registry.
pub const MIN_MAX_LOG_LINE_BYTES: usize = 256;
pub const MIN_MAX_LOG_BUFFER_PER_SERVICE_BYTES: usize = 4096;
pub const MIN_LOG_READ_BYTES_PER_EVENT: usize = 512;
pub const MIN_PRE_EVENTD_BUFFER_BYTES: usize = 4096;
pub const MIN_EVENTD_LOG_DATAGRAM_BYTES: usize = 4096;
pub const MAX_EVENTD_LOG_DATAGRAM_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryConfigWarning {
    NewerServicesSchemaVersion {
        observed: u32,
        supported: u32,
    },
    /// A log tuning knob was set below its minimum useful value. The
    /// compiled-in default is used instead.
    ///
    /// Kept rather than fatal, following the rule peinit already applies to
    /// the equivalent kernel command-line knobs (`init/model.rs`): "a typo in
    /// a logging knob must not decide how the machine boots". Warned rather
    /// than silent, because an operator who sets a value and sees no change
    /// has no way to tell the setting from their diagnosis.
    LogConfigValueBelowMinimum {
        key: &'static str,
        configured: u32,
        minimum: usize,
        using: usize,
    },
    EventdLogDatagramBytesOutOfRange {
        configured: u32,
        minimum: usize,
        maximum: usize,
        using: usize,
    },
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
            Self::LogConfigValueBelowMinimum {
                key,
                configured,
                minimum,
                using,
            } => write!(
                formatter,
                "Machine\\System\\Init\\{key} is {configured}, below the minimum {minimum}; using the default {using}",
            ),
            Self::EventdLogDatagramBytesOutOfRange {
                configured,
                minimum,
                maximum,
                using,
            } => write!(
                formatter,
                "Machine\\System\\eventd\\MaxLogDatagramBytes is {configured}, outside {minimum}..={maximum}; using the default {using}",
            ),
        }
    }
}

/// Read and validate all four log tuning knobs from the registry.
///
/// One reader shared by the boot path and `reload-config`, because they used
/// to have one each and the two had drifted: boot read all four keys, reload
/// read only `MaxLogLineLength` and `MaxLogBufferPerService` and left the
/// other two at their compiled-in defaults — so every reload silently reverted
/// them, whatever the registry said.
///
/// A value below its minimum keeps the default and produces a warning rather
/// than failing: see `RegistryConfigWarning::LogConfigValueBelowMinimum`.
///
/// Errors are returned as the raw `BoundaryError` because the two callers wrap
/// registry failures differently (`Phase2BootRunError` vs `ReloadConfigError`).
pub fn read_log_config_from_registry<R>(
    registry: &mut R,
) -> Result<(RuntimeLogConfig, Vec<RegistryConfigWarning>), BoundaryError>
where
    R: RegistryClient + ?Sized,
{
    let mut config = RuntimeLogConfig::default();
    let mut warnings = Vec::new();

    let mut accept = |key: &'static str,
                      configured: Option<u32>,
                      minimum: usize,
                      default: usize,
                      field: &mut usize| {
        let Some(configured) = configured else {
            return;
        };
        if (configured as usize) < minimum {
            warnings.push(RegistryConfigWarning::LogConfigValueBelowMinimum {
                key,
                configured,
                minimum,
                using: default,
            });
            return;
        }
        *field = configured as usize;
    };

    accept(
        MAX_LOG_LINE_LENGTH_FIELD,
        registry.read_max_log_line_length()?,
        MIN_MAX_LOG_LINE_BYTES,
        DEFAULT_MAX_LOG_LINE_BYTES,
        &mut config.max_line_bytes,
    );
    accept(
        MAX_LOG_BUFFER_PER_SERVICE_FIELD,
        registry.read_max_log_buffer_per_service()?,
        MIN_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
        DEFAULT_MAX_LOG_BUFFER_PER_SERVICE_BYTES,
        &mut config.max_buffer_per_service_bytes,
    );
    accept(
        LOG_READ_BYTES_PER_EVENT_FIELD,
        registry.read_log_read_bytes_per_event()?,
        MIN_LOG_READ_BYTES_PER_EVENT,
        DEFAULT_LOG_READ_BYTES_PER_EVENT,
        &mut config.read_bytes_per_event,
    );
    accept(
        PRE_EVENTD_BUFFER_FIELD,
        registry.read_pre_eventd_buffer_bytes()?,
        MIN_PRE_EVENTD_BUFFER_BYTES,
        DEFAULT_PRE_EVENTD_BUFFER_BYTES,
        &mut config.pre_eventd_buffer_bytes,
    );

    if let Some(configured) = registry.read_eventd_log_datagram_bytes()? {
        let configured = configured as usize;
        if (MIN_EVENTD_LOG_DATAGRAM_BYTES..=MAX_EVENTD_LOG_DATAGRAM_BYTES).contains(&configured) {
            config.eventd_log_datagram_bytes = configured;
        } else {
            warnings.push(RegistryConfigWarning::EventdLogDatagramBytesOutOfRange {
                configured: configured as u32,
                minimum: MIN_EVENTD_LOG_DATAGRAM_BYTES,
                maximum: MAX_EVENTD_LOG_DATAGRAM_BYTES,
                using: DEFAULT_EVENTD_LOG_DATAGRAM_BYTES,
            });
        }
    }

    Ok((config, warnings))
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

pub fn build_eventd_log_datagram_bytes_from_registry_values(
    values: &[RawRegistryValue],
) -> Option<u32> {
    values
        .iter()
        .find(|value| value.name == MAX_LOG_DATAGRAM_BYTES_FIELD)
        .and_then(|value| decode_dword_field(value, MAX_LOG_DATAGRAM_BYTES_FIELD).ok())
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

/// `Machine\\System\\Boot\\PostKillTimeout` — how long peinit waits for a
/// service cgroup to drain after SIGKILL before treating it as stuck. Sits
/// beside `ShutdownTimeout`, which bounds the shutdown as a whole; this bounds
/// one service's last stage of it.
pub fn build_post_kill_timeout_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    build_optional_boot_dword_from_registry_values(values, POST_KILL_TIMEOUT_FIELD)
}

/// `Machine\\System\\Boot\\SettleTimeout` — how long peinit waits for the boot
/// set to stop moving before starting `boot:settled` services anyway. Bounds
/// the wait so a service stuck in Starting delays a console prompt rather than
/// denying it.
pub fn build_settle_timeout_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    build_optional_boot_dword_from_registry_values(values, SETTLE_TIMEOUT_FIELD)
}

/// `Machine\\System\\Init\\LogReadBytesPerEvent` — how much peinit drains from
/// a service's stdout/stderr pipe per readable event. Larger favours throughput
/// on chatty services, smaller favours fairness between them.
pub fn build_log_read_bytes_per_event_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    build_optional_boot_dword_from_registry_values(values, LOG_READ_BYTES_PER_EVENT_FIELD)
}

/// `Machine\\System\\Init\\PreEventdBuffer` — total bytes of service output
/// peinit holds in memory before eventd is up to receive it. Raise it on a
/// system whose early services are noisy, lower it where boot-time memory is
/// tight; overflow drops the oldest output, it never blocks a service.
pub fn build_pre_eventd_buffer_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<Option<u32>, ServiceRegistryDecodeError> {
    build_optional_boot_dword_from_registry_values(values, PRE_EVENTD_BUFFER_FIELD)
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

/// The jobs socket bounds of PSPU §7.A, from `Machine\System\Init\`.
pub fn build_jobs_socket_limits_from_registry_values(
    values: &[RawRegistryValue],
) -> Result<JobsSocketLimits, ServiceRegistryDecodeError> {
    let defaults = JobsSocketLimits::default();
    Ok(JobsSocketLimits {
        max_connections: optional_init_dword(values, MAX_JOBS_CONNECTIONS_FIELD)?
            .map(|value| value as usize)
            .unwrap_or(defaults.max_connections),
        max_message_bytes: optional_init_dword(values, MAX_JOB_MESSAGE_SIZE_FIELD)?
            .map(|value| value as usize)
            .unwrap_or(defaults.max_message_bytes),
        connection_timeout_secs: optional_init_dword(values, JOBS_CONNECTION_TIMEOUT_FIELD)?
            .map(u64::from)
            .unwrap_or(defaults.connection_timeout_secs),
        max_jobs_per_submitter: optional_init_dword(values, MAX_JOBS_PER_SUBMITTER_FIELD)?
            .map(|value| value as usize)
            .unwrap_or(defaults.max_jobs_per_submitter),
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
