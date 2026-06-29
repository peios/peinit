use crate::control::lifecycle::{
    OnDemandStartDispatch, dispatch_on_demand_start_plan, plan_timer_start,
};
use crate::execution::graph::GraphContextId;
use crate::execution::start::StartExecutionDispatch;
use crate::ids::OperationId;
use crate::security::TokenSummary;
use crate::supervisor::relationships::gate_start_context;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimerStartDispatch {
    pub(super) requested_operation_id: OperationId,
    pub(super) outcome: OnDemandStartDispatch,
    pub(super) context_id: GraphContextId,
    pub(super) start_dispatches: Vec<StartExecutionDispatch>,
}

pub(super) fn dispatch_timer_start_for_work(
    work: &mut SupervisorWork,
    service: &str,
    observed_at_ns: u64,
    max_parallel_starts: u32,
) -> Result<TimerStartDispatch, SupervisorError> {
    let request_id = allocate_timer_request_id(work, observed_at_ns)?;
    let plan = plan_timer_start(&work.services, service).map_err(|source| {
        SupervisorError::Lifecycle(crate::control::lifecycle::LifecycleCommandError::StartPlan(
            source,
        ))
    })?;
    let start_services = plan
        .starts
        .iter()
        .map(|start| start.service.clone())
        .collect();
    let outcome = dispatch_on_demand_start_plan(
        &mut work.operations,
        &mut work.operation_ids,
        plan,
        request_id,
        None::<TokenSummary>,
        observed_at_ns,
    )
    .map_err(|source| {
        SupervisorError::Lifecycle(
            crate::control::lifecycle::LifecycleCommandError::StartDispatch(source),
        )
    })?;
    let context_id = work
        .graph
        .create_on_demand_context(&outcome, &work.services)
        .map_err(SupervisorError::GraphContext)?;
    let start_dispatches = gate_start_context(
        work,
        context_id,
        start_services,
        observed_at_ns,
        max_parallel_starts,
    )?;
    Ok(TimerStartDispatch {
        requested_operation_id: request_id,
        outcome,
        context_id,
        start_dispatches,
    })
}

fn allocate_timer_request_id(
    work: &mut SupervisorWork,
    observed_at_ns: u64,
) -> Result<OperationId, SupervisorError> {
    Ok(work
        .operation_ids
        .allocate_batch(1, observed_at_ns)
        .map_err(SupervisorError::RequestIdAllocation)?[0])
}
