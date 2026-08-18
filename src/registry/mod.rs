mod config;
mod fields;
mod provisioning;
mod service;
mod value;

#[cfg(feature = "peios-registry")]
mod lcs;

#[cfg(test)]
mod tests;

pub use config::{
    RegistryConfigWarning, SUPPORTED_SERVICES_SCHEMA_VERSION,
    build_boot_success_grace_from_registry_values, build_control_security_from_registry_values,
    build_control_socket_limits_from_registry_values,
    build_eventd_log_socket_path_from_registry_values,
    build_global_environment_from_registry_values,
    build_log_read_bytes_per_event_from_registry_values,
    build_max_log_buffer_per_service_from_registry_values,
    build_max_log_line_length_from_registry_values, build_max_parallel_starts_from_registry_values,
    build_post_kill_timeout_from_registry_values, build_pre_eventd_buffer_from_registry_values,
    build_settle_timeout_from_registry_values, build_shutdown_timeout_from_registry_values,
    services_schema_warnings,
};
pub use provisioning::{
    build_provisioned_path_from_registry_values, build_provisioned_path_registry_snapshot,
};
pub use service::{
    apply_inherited_service_security, build_service_definition_from_registry_values,
    build_service_security_from_registry_value,
};
pub use value::{
    RawRegistryValue, RegistryMultiStringDecodeError, RegistryStringDecodeError, RegistryValueType,
    ServiceRegistryDecodeError,
};

#[cfg(feature = "peios-registry")]
pub use lcs::{
    LcsRegistryClient, LcsRegistryReadError, LcsRegistryWatch, LcsRegistryWatches,
    LcsTimerLastRunWriter,
};

pub const GLOBAL_ENV_VARS_KEY: &str = r"Machine\System\Init\EnvVars";
pub const INIT_ROOT_KEY: &str = r"Machine\System\Init";
pub const EVENTD_ROOT_KEY: &str = r"Machine\System\eventd";
pub const SERVICES_ROOT_KEY: &str = r"Machine\System\Services";
pub const BOOT_ROOT_KEY: &str = r"Machine\System\Boot";
