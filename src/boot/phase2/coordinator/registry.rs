use crate::boundary::RegistryClient;
use crate::logging::RuntimeLogConfig;
use crate::shutdown::ShutdownSettings;

use super::{Phase2BootRunError, Phase2BootSettings, Phase2RecoveryReason};

pub(super) fn read_effective_log_config<R>(
    registry: &mut R,
) -> Result<RuntimeLogConfig, Phase2BootRunError>
where
    R: RegistryClient + ?Sized,
{
    let mut config = RuntimeLogConfig::default();
    if let Some(max_line_bytes) = registry
        .read_max_log_line_length()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?
    {
        config.max_line_bytes = max_line_bytes as usize;
    }
    if let Some(max_buffer_bytes) = registry
        .read_max_log_buffer_per_service()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?
    {
        config.max_buffer_per_service_bytes = max_buffer_bytes as usize;
    }
    Ok(config)
}

pub(super) fn read_effective_boot_settings<R>(
    mut settings: Phase2BootSettings,
    registry: &mut R,
) -> Result<Phase2BootSettings, Phase2BootRunError>
where
    R: RegistryClient + ?Sized,
{
    if let Some(max_parallel_starts) = registry
        .read_max_parallel_starts()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?
    {
        settings.max_parallel_starts = max_parallel_starts;
    }
    if let Some(boot_success_grace_secs) = registry
        .read_boot_success_grace_secs()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?
    {
        settings.boot_success_grace_secs = boot_success_grace_secs;
    }
    Ok(settings)
}

pub(super) fn read_effective_shutdown_settings<R>(
    registry: &mut R,
) -> Result<ShutdownSettings, Phase2BootRunError>
where
    R: RegistryClient + ?Sized,
{
    let mut settings = ShutdownSettings::default();
    if let Some(timeout_secs) = registry
        .read_shutdown_timeout_secs()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?
    {
        settings.global_timeout_secs = u64::from(timeout_secs);
    }
    Ok(settings)
}
