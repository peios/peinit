#[cfg(test)]
mod tests;

use crate::control::lifecycle::{
    OnDemandStartDispatch, OnDemandStartDispatchError, OnDemandStartPlanError,
    dispatch_on_demand_start_plan, plan_restart_policy_start,
};
use crate::ids::{IdAllocationError, OperationIdAllocator};
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;
use crate::service::runtime::ServiceState;

pub fn admit_due_restart_policy_start(
    services: &ServiceTable,
    operations: &mut OperationStore,
    operation_ids: &mut OperationIdAllocator,
    service: &str,
    now_ns: u64,
) -> Result<OnDemandStartDispatch, RestartPolicyAdmissionError> {
    ensure_due_restart_backoff(services, service, now_ns)?;
    let plan = plan_restart_policy_start(services, service)
        .map_err(RestartPolicyAdmissionError::StartPlan)?;

    let mut next_operation_ids = operation_ids.clone();
    let requested_id = next_operation_ids
        .allocate_batch(1, now_ns)
        .map_err(RestartPolicyAdmissionError::IdAllocation)?[0];
    let dispatch = dispatch_on_demand_start_plan(
        operations,
        &mut next_operation_ids,
        plan,
        requested_id,
        None,
        now_ns,
    )
    .map_err(RestartPolicyAdmissionError::StartDispatch)?;

    *operation_ids = next_operation_ids;
    Ok(dispatch)
}

pub(crate) fn ensure_due_restart_backoff(
    services: &ServiceTable,
    service: &str,
    now_ns: u64,
) -> Result<(), RestartPolicyAdmissionError> {
    let entry =
        services
            .get(service)
            .ok_or_else(|| RestartPolicyAdmissionError::UnknownService {
                service: service.to_string(),
            })?;
    if entry.definition_removed {
        return Err(RestartPolicyAdmissionError::DefinitionRemoved {
            service: service.to_string(),
        });
    }
    if entry.runtime.state != ServiceState::Backoff {
        return Err(RestartPolicyAdmissionError::NotInBackoff {
            service: service.to_string(),
            state: entry.runtime.state,
        });
    }
    let due_at_ns = entry.runtime.restart_backoff_until_ns.ok_or_else(|| {
        RestartPolicyAdmissionError::MissingBackoffDeadline {
            service: service.to_string(),
        }
    })?;
    if due_at_ns > now_ns {
        return Err(RestartPolicyAdmissionError::BackoffNotDue {
            service: service.to_string(),
            due_at_ns,
            now_ns,
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartPolicyAdmissionError {
    UnknownService {
        service: String,
    },
    DefinitionRemoved {
        service: String,
    },
    NotInBackoff {
        service: String,
        state: ServiceState,
    },
    MissingBackoffDeadline {
        service: String,
    },
    BackoffNotDue {
        service: String,
        due_at_ns: u64,
        now_ns: u64,
    },
    IdAllocation(IdAllocationError),
    StartPlan(OnDemandStartPlanError),
    StartDispatch(OnDemandStartDispatchError),
}
