mod closure;
mod dispatch;
mod model;
mod order;

use crate::operation::OperationSource;
use crate::service::ServiceTable;
use crate::service::runtime::TransitionCause;

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
    plan_start(
        services,
        service,
        OperationSource::Admin,
        TransitionCause::ExplicitStart,
    )
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
