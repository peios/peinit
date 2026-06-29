mod blocked;
mod planned;

use crate::ids::{OperationId, OperationIdAllocator};
use crate::operation::TokenSummary;
use crate::operation::store::OperationStore;

use self::blocked::dispatch_blocked_requested_start;
use self::planned::{
    dependency_start_count, dispatch_planned_starts,
    dispatch_planned_starts_with_existing_requested,
};
use super::model::{OnDemandStartDispatch, OnDemandStartDispatchError, OnDemandStartPlan};

pub fn dispatch_on_demand_start_plan(
    operations: &mut OperationStore,
    operation_ids: &mut OperationIdAllocator,
    plan: OnDemandStartPlan,
    requested_operation_id: OperationId,
    caller: Option<TokenSummary>,
    created_at_ns: u64,
) -> Result<OnDemandStartDispatch, OnDemandStartDispatchError> {
    let mut next_operations = operations.clone();
    let mut next_operation_ids = operation_ids.clone();
    let dependency_ids = next_operation_ids
        .allocate_batch(dependency_start_count(&plan), created_at_ns)
        .map_err(OnDemandStartDispatchError::IdAllocation)?;

    let dispatch = if plan.starts.is_empty() && !plan.blocked.is_empty() {
        dispatch_blocked_requested_start(
            &mut next_operations,
            plan.clone(),
            requested_operation_id,
            caller,
            created_at_ns,
        )?
    } else {
        dispatch_planned_starts(
            &mut next_operations,
            plan.clone(),
            requested_operation_id,
            caller,
            created_at_ns,
            dependency_ids,
        )?
    };

    *operations = next_operations;
    *operation_ids = next_operation_ids;
    Ok(dispatch)
}

pub fn dispatch_existing_requested_start_plan(
    operations: &mut OperationStore,
    operation_ids: &mut OperationIdAllocator,
    plan: OnDemandStartPlan,
    requested_operation_id: OperationId,
    created_at_ns: u64,
) -> Result<OnDemandStartDispatch, OnDemandStartDispatchError> {
    let mut next_operations = operations.clone();
    let mut next_operation_ids = operation_ids.clone();
    let dependency_ids = next_operation_ids
        .allocate_batch(dependency_start_count(&plan), created_at_ns)
        .map_err(OnDemandStartDispatchError::IdAllocation)?;
    let dispatch = dispatch_planned_starts_with_existing_requested(
        &mut next_operations,
        plan,
        requested_operation_id,
        created_at_ns,
        dependency_ids,
    )?;

    *operations = next_operations;
    *operation_ids = next_operation_ids;
    Ok(dispatch)
}
