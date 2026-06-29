use crate::job::JobStore;

use super::super::model::{
    PostStartHookTimeoutDispatch, StartExecutionContext, StartExecutionError,
};
use super::super::store::PostStartHookDeadline;
use super::completion::complete_post_start_operation;
use super::job::fail_timed_out_hook;

pub fn timeout_post_start_hook<P>(
    context: &mut StartExecutionContext<'_, P>,
    deadline: PostStartHookDeadline,
    now_ns: u64,
) -> Result<PostStartHookTimeoutDispatch, StartExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_graph = context.graph.clone();
    let mut next_jobs: JobStore = context.jobs.clone();
    let mut next_store = context.start_store.clone();

    next_store.remove_post_start_hook_deadline(deadline.operation_id);
    let mut sequence = next_store
        .remove_post_start_sequence(deadline.operation_id)
        .ok_or(StartExecutionError::MissingPostStartHookSequence {
            operation_id: deadline.operation_id,
        })?;
    sequence.record_failure();
    context
        .controller
        .kill_cgroup(&deadline.hooks_cgroup_id)
        .map_err(StartExecutionError::Boundary)?;
    let job_event = fail_timed_out_hook(&mut next_jobs, deadline.job_id, now_ns)?;
    let completion = complete_post_start_operation(
        &mut next_services,
        &mut next_operations,
        &mut next_graph,
        &sequence,
        now_ns,
    )?;

    *context.services = next_services;
    *context.operations = next_operations;
    *context.graph = next_graph;
    *context.jobs = next_jobs;
    *context.start_store = next_store;

    Ok(PostStartHookTimeoutDispatch {
        job_event,
        operation_events: completion.operation_events,
        service_transitions: completion.service_transitions,
        graph_events: completion.graph_events,
        killed_cgroup_id: deadline.hooks_cgroup_id,
        timed_out_at_ns: now_ns,
    })
}
