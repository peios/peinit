use crate::ids::OperationId;
use crate::operation::store::{OperationRequest, OperationStore};
use crate::operation::{OperationType, TokenSummary};

use super::super::model::{
    OnDemandStartDispatch, OnDemandStartDispatchError, OnDemandStartPlan, StartBlockReason,
};

pub(super) fn dispatch_blocked_requested_start(
    operations: &mut OperationStore,
    plan: OnDemandStartPlan,
    requested_operation_id: OperationId,
    caller: Option<TokenSummary>,
    created_at_ns: u64,
) -> Result<OnDemandStartDispatch, OnDemandStartDispatchError> {
    let request = OperationRequest {
        id: requested_operation_id,
        operation_type: OperationType::Start,
        service: plan.requested.clone(),
        source: plan.requested_operation_source,
        caller,
        created_at_ns,
    };
    let requested_operation = operations
        .request_operation(request)
        .map_err(OnDemandStartDispatchError::OperationStore)?;
    let mut events = requested_operation.events.clone();
    if requested_operation.returned_operation_id == requested_operation_id {
        events.push(
            operations
                .fail_operation(
                    requested_operation_id,
                    created_at_ns,
                    blocked_failure_reason(&plan),
                )
                .map_err(OnDemandStartDispatchError::OperationStore)?,
        );
    }

    Ok(OnDemandStartDispatch {
        plan,
        requested_operation,
        dependency_operations: Vec::new(),
        events,
    })
}

fn blocked_failure_reason(plan: &OnDemandStartPlan) -> String {
    plan.blocked
        .iter()
        .find(|blocked| blocked.service == plan.requested)
        .map(|blocked| match &blocked.reason {
            StartBlockReason::HardDependencyUnavailable { target, kind, .. } => {
                format!("DependencyFailure: {kind:?} dependency {target} is unavailable")
            }
            StartBlockReason::HardDependencyBlocked { target, kind } => {
                format!("DependencyFailure: {kind:?} dependency {target} is blocked")
            }
        })
        .unwrap_or_else(|| "DependencyFailure: requested service is blocked".to_string())
}
