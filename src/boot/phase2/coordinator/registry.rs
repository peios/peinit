use crate::boundary::RegistryClient;
use crate::logging::RuntimeLogConfig;
use crate::registry::{RegistryConfigWarning, read_log_config_from_registry};
use crate::shutdown::ShutdownSettings;

use super::{Phase2BootRunError, Phase2BootSettings, Phase2RecoveryReason};

pub(super) fn read_effective_log_config<R>(
    registry: &mut R,
) -> Result<(RuntimeLogConfig, Vec<RegistryConfigWarning>), Phase2BootRunError>
where
    R: RegistryClient + ?Sized,
{
    read_log_config_from_registry(registry)
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)
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
    if let Some(settle_timeout_secs) = registry
        .read_settle_timeout_secs()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?
    {
        settings.settle_timeout_secs = settle_timeout_secs;
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
    if let Some(timeout_secs) = registry
        .read_post_kill_timeout_secs()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?
    {
        settings.post_kill_timeout_secs = u64::from(timeout_secs);
    }
    Ok(settings)
}
