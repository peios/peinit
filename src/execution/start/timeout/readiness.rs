use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::operation::operation_timeout_result;
use crate::service::runtime::TransitionCause;

use crate::execution::start::{
    ReadinessDeadline, ReadinessTimeoutDispatch, StartExecutionContext, StartExecutionError,
};

pub fn timeout_readiness<P>(
    context: &mut StartExecutionContext<'_, P>,
    deadline: ReadinessDeadline,
    now_ns: u64,
) -> Result<ReadinessTimeoutDispatch, StartExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_graph = context.graph.clone();
    let mut next_store = context.start_store.clone();

    next_store.remove_readiness_deadline(deadline.operation_id);
    context
        .controller
        .kill_cgroup(&deadline.service_cgroup_id)
        .map_err(StartExecutionError::Boundary)?;
    let failure = apply_start_failure(
        &mut next_services,
        &mut next_operations,
        &mut next_graph,
        StartFailureRequest {
            service: deadline.service,
            operation_id: deadline.operation_id,
            failed_at_ns: now_ns,
            failure_cause: TransitionCause::ReadinessTimeout,
            reason: operation_timeout_result("start timed out waiting for READY=1"),
        },
    )
    .map_err(StartExecutionError::StartFailure)?;

    *context.services = next_services;
    *context.operations = next_operations;
    *context.graph = next_graph;
    *context.start_store = next_store;

    Ok(ReadinessTimeoutDispatch {
        operation_events: failure.operation_events,
        service_transitions: failure.service_transitions,
        graph_events: failure.graph_events,
        killed_cgroup_id: deadline.service_cgroup_id,
        timed_out_at_ns: now_ns,
    })
}
