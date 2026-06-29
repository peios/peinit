use crate::job::{JobEvent, JobStore};
use crate::operation::store::OperationStore;
use crate::service::ServiceTable;

use super::super::deadline::post_start_hook_deadline;
use super::super::model::{
    PostStartHookTerminalDispatch, StartExecutionContext, StartExecutionError,
};
use super::super::store::{PostStartHookSequence, StartExecutionStore};
use super::completion::complete_post_start_operation;
use super::job::{
    ended_at_ns, operation_id, post_start_hook_job, post_start_hook_succeeded,
    validate_post_start_hook_terminal_event,
};

pub fn complete_post_start_hook_job<P>(
    context: &mut StartExecutionContext<'_, P>,
    job_event: JobEvent,
) -> Result<PostStartHookTerminalDispatch, StartExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    validate_post_start_hook_terminal_event(&job_event)?;
    let operation_id = operation_id(&job_event)?;
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_graph = context.graph.clone();
    let mut next_jobs = context.jobs.clone();
    let mut next_job_ids = context.job_ids.clone();
    let mut next_store = context.start_store.clone();

    next_store.remove_post_start_hook_deadline(operation_id);
    let sequence = next_store
        .post_start_sequence(operation_id)
        .cloned()
        .ok_or(StartExecutionError::MissingPostStartHookSequence { operation_id })?;
    let ended_at_ns = ended_at_ns(&job_event)?;
    let dispatch = apply_post_start_hook_terminal(
        &mut PostStartTerminalContext {
            services: &mut next_services,
            operations: &mut next_operations,
            graph: &mut next_graph,
            jobs: &mut next_jobs,
            job_ids: &mut next_job_ids,
            start_store: &mut next_store,
            controller: context.controller,
        },
        job_event,
        sequence,
        ended_at_ns,
    )?;

    *context.services = next_services;
    *context.operations = next_operations;
    *context.graph = next_graph;
    *context.jobs = next_jobs;
    *context.job_ids = next_job_ids;
    *context.start_store = next_store;

    Ok(dispatch)
}

struct PostStartTerminalContext<'a, P>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    services: &'a mut ServiceTable,
    operations: &'a mut OperationStore,
    graph: &'a mut crate::execution::graph::GraphExecutionStore,
    jobs: &'a mut JobStore,
    job_ids: &'a mut crate::ids::JobIdAllocator,
    start_store: &'a mut StartExecutionStore,
    controller: &'a mut P,
}

fn apply_post_start_hook_terminal<P>(
    context: &mut PostStartTerminalContext<'_, P>,
    job_event: JobEvent,
    mut sequence: PostStartHookSequence,
    ended_at_ns: u64,
) -> Result<PostStartHookTerminalDispatch, StartExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    if !post_start_hook_succeeded(&job_event) {
        sequence.record_failure();
    }
    if let Some(command) = sequence.next_command() {
        return create_next_post_start_hook(context, job_event, sequence, command, ended_at_ns);
    }

    context
        .start_store
        .remove_post_start_sequence(sequence.operation_id);
    let hooks_cgroup_id = post_start_hook_deadline(&sequence, job_event.job_id).hooks_cgroup_id;
    context
        .controller
        .kill_cgroup(&hooks_cgroup_id)
        .map_err(StartExecutionError::Boundary)?;
    let completion = complete_post_start_operation(
        context.services,
        context.operations,
        context.graph,
        &sequence,
        ended_at_ns,
    )?;

    Ok(PostStartHookTerminalDispatch {
        job_event,
        next_job_event: None,
        operation_events: completion.operation_events,
        service_transitions: completion.service_transitions,
        graph_events: completion.graph_events,
        killed_cgroup_id: Some(hooks_cgroup_id),
    })
}

fn create_next_post_start_hook<P>(
    context: &mut PostStartTerminalContext<'_, P>,
    job_event: JobEvent,
    mut sequence: PostStartHookSequence,
    command: Vec<String>,
    created_at_ns: u64,
) -> Result<PostStartHookTerminalDispatch, StartExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let job_id = context
        .job_ids
        .allocate_batch(1, created_at_ns)
        .map(|ids| ids[0])
        .map_err(StartExecutionError::JobIdAllocation)?;
    let hook_index = sequence.next_index;
    let job = post_start_hook_job(&sequence, job_id, command, hook_index, created_at_ns)?;
    let next_job_event = context
        .jobs
        .create_job(job)
        .map_err(StartExecutionError::JobStore)?;
    sequence.advance();
    context
        .start_store
        .record_post_start_sequence(sequence.clone());
    context
        .start_store
        .record_post_start_hook_deadline(post_start_hook_deadline(&sequence, job_id));

    Ok(PostStartHookTerminalDispatch {
        job_event,
        next_job_event: Some(next_job_event),
        operation_events: Vec::new(),
        service_transitions: Vec::new(),
        graph_events: Vec::new(),
        killed_cgroup_id: None,
    })
}
