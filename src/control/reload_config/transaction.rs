use crate::boundary::RegistryClient;
use crate::logging::RuntimeLogConfig;
use crate::registry::services_schema_warnings;
use crate::registry::{RegistryConfigWarning, read_log_config_from_registry};
use crate::service::{ServiceTable, validate_service_graph};
use crate::shutdown::ShutdownSettings;

use super::model::{ReloadConfigError, ReloadConfigOutcome};

pub fn reload_config<R>(
    registry: &mut R,
    services: &mut ServiceTable,
) -> Result<ReloadConfigOutcome, ReloadConfigError>
where
    R: RegistryClient + ?Sized,
{
    let services_schema_version = registry
        .read_services_schema_version()
        .map_err(ReloadConfigError::Registry)?;
    let definitions = registry
        .read_service_definitions()
        .map_err(ReloadConfigError::Registry)?;
    let control_security = registry
        .read_control_security()
        .map_err(ReloadConfigError::Registry)?;
    let control_limits = registry
        .read_control_socket_limits()
        .map_err(ReloadConfigError::Registry)?;
    let jobs_limits = registry
        .read_jobs_socket_limits()
        .map_err(ReloadConfigError::Registry)?;
    let (log_config, log_config_warnings) = read_log_config(registry)?;
    let shutdown_settings = read_shutdown_settings(registry)?;
    let global_environment = registry
        .read_global_environment()
        .map_err(ReloadConfigError::Registry)?;
    let eventd_log_socket_path = registry.read_eventd_log_socket_path().unwrap_or(None);
    let validation = validate_service_graph(&definitions).map_err(ReloadConfigError::Validation)?;
    let mut next_services = services.clone();
    let summary = next_services
        .apply_definition_snapshot(definitions)
        .map_err(ReloadConfigError::ServiceTable)?;

    *services = next_services;
    Ok(ReloadConfigOutcome {
        summary,
        services_schema_version,
        config_warnings: services_schema_warnings(services_schema_version)
            .into_iter()
            .chain(log_config_warnings)
            .collect(),
        control_security,
        control_limits,
        jobs_limits,
        log_config,
        shutdown_settings,
        global_environment,
        eventd_log_socket_path,
        warnings: validation.warnings,
    })
}

fn read_log_config<R>(
    registry: &mut R,
) -> Result<(RuntimeLogConfig, Vec<RegistryConfigWarning>), ReloadConfigError>
where
    R: RegistryClient + ?Sized,
{
    read_log_config_from_registry(registry).map_err(ReloadConfigError::Registry)
}

fn read_shutdown_settings<R>(registry: &mut R) -> Result<ShutdownSettings, ReloadConfigError>
where
    R: RegistryClient + ?Sized,
{
    let mut settings = ShutdownSettings::default();
    if let Some(timeout_secs) = registry
        .read_shutdown_timeout_secs()
        .map_err(ReloadConfigError::Registry)?
    {
        settings.global_timeout_secs = u64::from(timeout_secs);
    }
    Ok(settings)
}
