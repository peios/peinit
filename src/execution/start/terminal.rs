use crate::boundary::ProcessController;
use crate::job::JobEvent;

use super::event::{
    ended_at_ns, operation_id, pre_start_hook_failure_reason, pre_start_hook_succeeded, service,
    validate_pre_start_hook_terminal_event,
};
use super::model::{PreStartHookTerminalDispatch, StartExecutionContext, StartExecutionError};
use super::terminal_apply::{
    PreStartTerminalContext, apply_pre_start_hook_failure, apply_pre_start_hook_success,
};

pub fn complete_pre_start_hook_job<P>(
    context: &mut StartExecutionContext<'_, P>,
    job_event: JobEvent,
) -> Result<PreStartHookTerminalDispatch, StartExecutionError>
where
    P: ProcessController + ?Sized,
{
    validate_pre_start_hook_terminal_event(&job_event)?;
    let service = service(&job_event)?;
    let operation_id = operation_id(&job_event, &service)?;

    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_graph = context.graph.clone();
    let mut next_jobs = context.jobs.clone();
    let mut next_job_ids = context.job_ids.clone();
    let mut next_store = context.start_store.clone();

    next_store.remove_pre_start_hook_deadline(operation_id);
    let sequence = next_store
        .pre_start_sequence(operation_id)
        .cloned()
        .ok_or(StartExecutionError::MissingPreStartHookSequence { operation_id })?;
    let ended_at_ns = ended_at_ns(&job_event)?;
    let mut terminal_context = PreStartTerminalContext {
        services: &mut next_services,
        operations: &mut next_operations,
        graph: &mut next_graph,
        jobs: &mut next_jobs,
        job_ids: &mut next_job_ids,
        start_store: &mut next_store,
        controller: context.controller,
    };

    let dispatch = if pre_start_hook_succeeded(&job_event) {
        apply_pre_start_hook_success(&mut terminal_context, job_event, sequence, ended_at_ns)?
    } else {
        apply_pre_start_hook_failure(
            &mut terminal_context,
            job_event,
            sequence,
            ended_at_ns,
            pre_start_hook_failure_reason,
        )?
    };

    *context.services = next_services;
    *context.operations = next_operations;
    *context.graph = next_graph;
    *context.jobs = next_jobs;
    *context.job_ids = next_job_ids;
    *context.start_store = next_store;

    Ok(dispatch)
}
