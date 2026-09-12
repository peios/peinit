use crate::ids::OperationId;
use crate::operation::store::{OperationRequest, OperationRequestOutcome, OperationStore};
use crate::operation::{OperationSource, OperationType, TokenSummary};

use super::super::model::{
    OnDemandStartDispatch, OnDemandStartDispatchError, OnDemandStartPlan, PlannedStart,
};

pub(super) fn dispatch_planned_starts(
    operations: &mut OperationStore,
    plan: OnDemandStartPlan,
    requested_operation_id: OperationId,
    caller: Option<TokenSummary>,
    created_at_ns: u64,
    dependency_ids: Vec<OperationId>,
) -> Result<OnDemandStartDispatch, OnDemandStartDispatchError> {
    let mut dependency_ids = dependency_ids.into_iter();
    let mut events = Vec::new();
    let mut dependency_operations = Vec::new();
    let mut requested_operation = None;

    for start in &plan.starts {
        let id = if start.service == plan.requested {
            requested_operation_id
        } else {
            dependency_ids.next().ok_or_else(|| {
                OnDemandStartDispatchError::MissingDependencyOperationId {
                    service: start.service.clone(),
                }
            })?
        };
        let outcome = request_start(
            operations,
            id,
            start,
            caller_for_start(&caller, start),
            created_at_ns,
        )?;
        events.extend(outcome.events.clone());
        if start.service == plan.requested {
            requested_operation = Some(outcome);
        } else {
            dependency_operations.push(outcome);
        }
    }

    let requested_operation =
        requested_operation.ok_or_else(|| OnDemandStartDispatchError::MissingRequestedStart {
            service: plan.requested.clone(),
        })?;
    Ok(OnDemandStartDispatch {
        plan,
        requested_operation,
        dependency_operations,
        events,
    })
}

pub(super) fn dispatch_planned_starts_with_existing_requested(
    operations: &mut OperationStore,
    plan: OnDemandStartPlan,
    requested_operation_id: OperationId,
    created_at_ns: u64,
    dependency_ids: Vec<OperationId>,
) -> Result<OnDemandStartDispatch, OnDemandStartDispatchError> {
    let mut dependency_ids = dependency_ids.into_iter();
    let mut events = Vec::new();
    let mut dependency_operations = Vec::new();
    let mut requested_operation = None;

    for start in &plan.starts {
        if start.service == plan.requested {
            requested_operation = Some(existing_requested_start(
                operations,
                requested_operation_id,
                &plan.requested,
            )?);
            continue;
        }
        let id = dependency_ids.next().ok_or_else(|| {
            OnDemandStartDispatchError::MissingDependencyOperationId {
                service: start.service.clone(),
            }
        })?;
        let outcome = request_start(operations, id, start, None, created_at_ns)?;
        events.extend(outcome.events.clone());
        dependency_operations.push(outcome);
    }

    let requested_operation =
        requested_operation.ok_or_else(|| OnDemandStartDispatchError::MissingRequestedStart {
            service: plan.requested.clone(),
        })?;
    Ok(OnDemandStartDispatch {
        plan,
        requested_operation,
        dependency_operations,
        events,
    })
}

pub(super) fn dependency_start_count(plan: &OnDemandStartPlan) -> usize {
    plan.starts
        .iter()
        .filter(|start| start.service != plan.requested)
        .count()
}

fn request_start(
    operations: &mut OperationStore,
    id: OperationId,
    start: &PlannedStart,
    caller: Option<TokenSummary>,
    created_at_ns: u64,
) -> Result<OperationRequestOutcome, OnDemandStartDispatchError> {
    operations
        .request_operation(OperationRequest {
            id,
            operation_type: OperationType::Start,
            service: start.service.clone(),
            source: start.operation_source,
            caller,
            created_at_ns,
        })
        .map_err(OnDemandStartDispatchError::OperationStore)
}

fn existing_requested_start(
    operations: &OperationStore,
    id: OperationId,
    service: &str,
) -> Result<OperationRequestOutcome, OnDemandStartDispatchError> {
    let operation = operations
        .get(id)
        .ok_or(crate::operation::store::OperationStoreError::UnknownOperation { id })
        .map_err(OnDemandStartDispatchError::OperationStore)?;
    if !matches!(
        operation.operation_type,
        OperationType::Start | OperationType::Restart
    ) || operation.service != service
        || operation.state != crate::operation::OperationState::Pending
    {
        return Err(OnDemandStartDispatchError::OperationStore(
            crate::operation::store::OperationStoreError::InvalidEventRecord {
                id,
                state: operation.state,
                reason: "a deferred backoff operation must be a pending start or restart for the requested service",
            },
        ));
    }
    Ok(OperationRequestOutcome {
        returned_operation_id: id,
        stored_operation_id: id,
        decision: crate::operation::conflict::OperationConflictDecision::CreateNew,
        events: Vec::new(),
    })
}

fn caller_for_start(caller: &Option<TokenSummary>, start: &PlannedStart) -> Option<TokenSummary> {
    (start.operation_source == OperationSource::Admin)
        .then(|| caller.clone())
        .flatten()
}
