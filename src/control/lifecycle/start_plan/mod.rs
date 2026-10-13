mod closure;
mod dispatch;
mod model;
mod order;

use crate::operation::OperationSource;
use crate::service::runtime::TransitionCause;
use crate::service::{ServiceGraphFinding, ServiceTable, validate_service_graph};

use closure::collect_start_closure;
use order::dependency_order;

pub use dispatch::{dispatch_existing_requested_start_plan, dispatch_on_demand_start_plan};
pub use model::{
    DependencyAvailability, OnDemandStartDispatch, OnDemandStartDispatchError, OnDemandStartPlan,
    OnDemandStartPlanError, PlannedStart, StartBlockReason, StartPlanBlockedService,
};

pub fn plan_on_demand_start(
    services: &ServiceTable,
    service: &str,
) -> Result<OnDemandStartPlan, OnDemandStartPlanError> {
    plan_queued_start(services, service, OperationSource::Admin)
}

/// A promoted operation retains its source. Only administrator requests gain
/// the definition-validation refusal; internal work keeps its existing checks.
pub(crate) fn plan_queued_start(
    services: &ServiceTable,
    service: &str,
    source: OperationSource,
) -> Result<OnDemandStartPlan, OnDemandStartPlanError> {
    plan_start(services, service, source, TransitionCause::ExplicitStart)
}

pub fn plan_restart_policy_start(
    services: &ServiceTable,
    service: &str,
) -> Result<OnDemandStartPlan, OnDemandStartPlanError> {
    plan_start(
        services,
        service,
        OperationSource::RestartPolicy,
        TransitionCause::RestartPolicy,
    )
}

pub fn plan_timer_start(
    services: &ServiceTable,
    service: &str,
) -> Result<OnDemandStartPlan, OnDemandStartPlanError> {
    plan_start(
        services,
        service,
        OperationSource::Timer,
        TransitionCause::Timer,
    )
}

pub fn plan_binds_to_recovery_start(
    services: &ServiceTable,
    service: &str,
) -> Result<OnDemandStartPlan, OnDemandStartPlanError> {
    plan_start(
        services,
        service,
        OperationSource::BindsToRecovery,
        TransitionCause::BindsToRecovery,
    )
}

pub fn plan_on_failure_start(
    services: &ServiceTable,
    service: &str,
) -> Result<OnDemandStartPlan, OnDemandStartPlanError> {
    plan_start(
        services,
        service,
        OperationSource::OnFailure,
        TransitionCause::ExplicitStart,
    )
}

/// Plan the start of a service whose terminal has just come free.
///
/// `ExplicitStart` as the cause, like `OnFailure`, and for one reason that
/// matters: the waiter is almost always sitting in Skipped from the moment it
/// lost the terminal, and `Skipped -> Inactive` is only travelled on an
/// explicit start. Any other cause would reach the activation still Skipped
/// and die on an invalid transition — the service would be woken and then
/// refused, every time.
pub fn plan_tty_release_start(
    services: &ServiceTable,
    service: &str,
) -> Result<OnDemandStartPlan, OnDemandStartPlanError> {
    plan_start(
        services,
        service,
        OperationSource::TtyRelease,
        TransitionCause::ExplicitStart,
    )
}

fn plan_start(
    services: &ServiceTable,
    service: &str,
    requested_operation_source: OperationSource,
    requested_transition_cause: TransitionCause,
) -> Result<OnDemandStartPlan, OnDemandStartPlanError> {
    let target = services
        .get(service)
        .ok_or_else(|| OnDemandStartPlanError::UnknownService {
            service: service.to_string(),
        })?;
    if target.definition_removed {
        return Err(OnDemandStartPlanError::DefinitionRemoved {
            service: service.to_string(),
        });
    }

    let closure = collect_start_closure(services, service);
    let starts = if closure.blocked.contains_key(service) {
        Vec::new()
    } else {
        let startable = closure.startable_services();
        // Internal recovery must not roll back a source service's terminal
        // transition because its target has an invalid definition.
        if requested_operation_source == OperationSource::Admin {
            // Validate exactly the definitions this plan would activate. An
            // already-satisfied dependency and an unrelated invalid definition
            // must not reject an otherwise valid start. Missing/disabled hard
            // dependencies and cycles retain the closure/order handling below;
            // conflicting boot triggers are not a constraint on an explicit start.
            let definitions = startable
                .iter()
                .filter_map(|name| services.definition(name).cloned())
                .collect::<Vec<_>>();
            if let Err(failure) = validate_service_graph(&definitions) {
                let findings = failure
                    .findings
                    .into_iter()
                    .filter(|finding| {
                        matches!(
                            finding,
                            ServiceGraphFinding::InvalidHealthCheckRestartWindow { .. }
                                | ServiceGraphFinding::UnschedulableHealthCheck { .. }
                                | ServiceGraphFinding::InvalidTimerSchedule { .. }
                        )
                    })
                    .collect::<Vec<_>>();
                if !findings.is_empty() {
                    return Err(OnDemandStartPlanError::Validation { findings });
                }
            }
        }
        dependency_order(&startable, services)?
            .into_iter()
            .map(|planned| {
                planned_start(
                    planned,
                    service,
                    requested_operation_source,
                    requested_transition_cause,
                )
            })
            .collect()
    };

    Ok(OnDemandStartPlan {
        requested: service.to_string(),
        requested_operation_source,
        requested_transition_cause,
        starts,
        blocked: closure.blocked.into_values().collect(),
    })
}

fn planned_start(
    service: String,
    requested: &str,
    requested_operation_source: OperationSource,
    requested_transition_cause: TransitionCause,
) -> PlannedStart {
    if service == requested {
        PlannedStart {
            service,
            operation_source: requested_operation_source,
            transition_cause: requested_transition_cause,
        }
    } else {
        PlannedStart {
            service,
            operation_source: OperationSource::DependencyPropagation,
            transition_cause: TransitionCause::DependencyStart,
        }
    }
}
