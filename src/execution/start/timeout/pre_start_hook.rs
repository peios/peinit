use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::operation::operation_timeout_result;
use crate::service::runtime::TransitionCause;

use crate::execution::start::{
    PreStartHookDeadline, PreStartHookTimeoutDispatch, StartExecutionContext, StartExecutionError,
};

use super::job::fail_timed_out_job;

pub fn timeout_pre_start_hook<P>(
    context: &mut StartExecutionContext<'_, P>,
    deadline: PreStartHookDeadline,
    now_ns: u64,
) -> Result<PreStartHookTimeoutDispatch, StartExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_graph = context.graph.clone();
    let mut next_jobs = context.jobs.clone();
    let mut next_store = context.start_store.clone();

    next_store.remove_pre_start_hook_deadline(deadline.operation_id);
    let sequence = next_store
        .remove_pre_start_sequence(deadline.operation_id)
        .ok_or(StartExecutionError::MissingPreStartHookSequence {
            operation_id: deadline.operation_id,
        })?;
    context
        .controller
        .kill_cgroup(&deadline.service_cgroup_id)
        .map_err(StartExecutionError::Boundary)?;
    let job_event = fail_timed_out_job(
        &mut next_jobs,
        deadline.job_id,
        now_ns,
        "pre-start hook timed out before launch",
        "pre-start hook timed out",
    )?;
    let failure = apply_start_failure(
        &mut next_services,
        &mut next_operations,
        &mut next_graph,
        StartFailureRequest {
            service: sequence.service,
            operation_id: sequence.operation_id,
            failed_at_ns: now_ns,
            failure_cause: TransitionCause::PreHookFailure,
            reason: operation_timeout_result("ExecStartPre command timed out"),
        },
    )
    .map_err(StartExecutionError::StartFailure)?;

    *context.services = next_services;
    *context.operations = next_operations;
    *context.graph = next_graph;
    *context.jobs = next_jobs;
    *context.start_store = next_store;

    Ok(PreStartHookTimeoutDispatch {
        job_event,
        operation_events: failure.operation_events,
        service_transitions: failure.service_transitions,
        graph_events: failure.graph_events,
        killed_cgroup_id: deadline.service_cgroup_id,
        timed_out_at_ns: now_ns,
    })
}
