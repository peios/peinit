use crate::boundary::{Clock, RegistryClient};
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::operation::store::OperationStore;
use crate::registry::services_schema_warnings;
use crate::service::ServiceTable;

use super::planner::prepare_phase2_boot_plan_with_retained;

mod model;
mod registry;

pub use model::{
    DEFAULT_BOOT_SUCCESS_GRACE_SECS, DEFAULT_MAX_PARALLEL_STARTS, Phase2BootRun,
    Phase2BootRunError, Phase2BootSettings, Phase2RecoveryReason,
};
use registry::{
    read_effective_boot_settings, read_effective_log_config, read_effective_shutdown_settings,
};

pub fn run_phase2_boot<R, C>(
    settings: Phase2BootSettings,
    registry: &mut R,
    clock: &mut C,
    operation_ids: &mut OperationIdAllocator,
    job_ids: &mut JobIdAllocator,
    operations: &mut OperationStore,
) -> Result<Phase2BootRun, Phase2BootRunError>
where
    R: RegistryClient + ?Sized,
    C: Clock + ?Sized,
{
    run_phase2_boot_with_retained(
        settings,
        registry,
        clock,
        operation_ids,
        job_ids,
        operations,
        &[],
    )
}

pub(crate) fn run_phase2_boot_with_retained<R, C>(
    settings: Phase2BootSettings,
    registry: &mut R,
    clock: &mut C,
    operation_ids: &mut OperationIdAllocator,
    job_ids: &mut JobIdAllocator,
    operations: &mut OperationStore,
    retained_satisfied: &[String],
) -> Result<Phase2BootRun, Phase2BootRunError>
where
    R: RegistryClient + ?Sized,
    C: Clock + ?Sized,
{
    if settings.max_parallel_starts == 0 {
        return Err(Phase2BootRunError::RecoveryRequired(
            Phase2RecoveryReason::InvalidMaxParallelStarts,
        ));
    }
    let settings = read_effective_boot_settings(settings, registry)?;
    if settings.max_parallel_starts == 0 {
        return Err(Phase2BootRunError::RecoveryRequired(
            Phase2RecoveryReason::InvalidMaxParallelStarts,
        ));
    }
    let shutdown_settings = read_effective_shutdown_settings(registry)?;

    let services = registry
        .read_service_definitions()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?;
    let services_schema_version = registry
        .read_services_schema_version()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?;
    let control_security = registry
        .read_control_security()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?;
    let control_limits = registry
        .read_control_socket_limits()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?;
    let log_config = read_effective_log_config(registry)?;
    let global_environment = registry
        .read_global_environment()
        .map_err(Phase2RecoveryReason::RegistryRead)
        .map_err(Phase2BootRunError::RecoveryRequired)?;
    let eventd_log_socket_path = registry.read_eventd_log_socket_path().unwrap_or(None);
    let observed_at_ns = clock
        .monotonic_ns()
        .map_err(Phase2RecoveryReason::Clock)
        .map_err(Phase2BootRunError::RecoveryRequired)?;

    let mut next_operation_ids = operation_ids.clone();
    let mut next_job_ids = job_ids.clone();
    let plan = prepare_phase2_boot_plan_with_retained(
        settings.mode,
        &services,
        settings.max_parallel_starts,
        observed_at_ns,
        &mut next_operation_ids,
        &mut next_job_ids,
        retained_satisfied,
    )
    .map_err(Phase2BootRunError::Plan)?;

    let mut next_operations = operations.clone();
    let dispatch = next_operations
        .dispatch_phase2_boot_plan(&plan)
        .map_err(Phase2BootRunError::Dispatch)?;
    let service_table =
        ServiceTable::from_boot_snapshot(services).map_err(Phase2BootRunError::ServiceTable)?;

    *operation_ids = next_operation_ids;
    *job_ids = next_job_ids;
    *operations = next_operations;

    Ok(Phase2BootRun {
        settings,
        services_schema_version,
        config_warnings: services_schema_warnings(services_schema_version),
        shutdown_settings,
        control_security,
        control_limits,
        log_config,
        service_table,
        global_environment,
        eventd_log_socket_path,
        plan,
        dispatch,
    })
}
