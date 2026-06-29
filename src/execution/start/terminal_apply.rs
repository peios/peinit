use crate::boundary::ProcessController;
use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::execution::graph::GraphExecutionStore;
use crate::ids::JobIdAllocator;
use crate::job::{JobEvent, JobRecord, JobStore, ServiceHookJobSpec, ServiceMainJobSpec};
use crate::operation::store::OperationStore;
use crate::security::{TokenSummary, hook_execution_identity};
use crate::service::ServiceTable;
use crate::service::runtime::TransitionCause;

use super::deadline::{pre_start_hook_deadline, readiness_deadline};
use super::model::{PreStartHookTerminalDispatch, StartExecutionError};
use super::store::{PreStartHookSequence, StartExecutionStore};

pub(super) struct PreStartTerminalContext<'a, P>
where
    P: ProcessController + ?Sized,
{
    pub services: &'a mut ServiceTable,
    pub operations: &'a mut OperationStore,
    pub graph: &'a mut GraphExecutionStore,
    pub jobs: &'a mut JobStore,
    pub job_ids: &'a mut JobIdAllocator,
    pub start_store: &'a mut StartExecutionStore,
    pub controller: &'a mut P,
}

pub(super) fn apply_pre_start_hook_success<P>(
    context: &mut PreStartTerminalContext<'_, P>,
    job_event: JobEvent,
    sequence: PreStartHookSequence,
    created_at_ns: u64,
) -> Result<PreStartHookTerminalDispatch, StartExecutionError>
where
    P: ProcessController + ?Sized,
{
    if let Some(command) = sequence.next_command() {
        return create_next_hook(context, job_event, sequence, command, created_at_ns);
    }
    create_main_after_hooks(context, job_event, sequence, created_at_ns)
}

pub(super) fn apply_pre_start_hook_failure<P>(
    context: &mut PreStartTerminalContext<'_, P>,
    job_event: JobEvent,
    sequence: PreStartHookSequence,
    failed_at_ns: u64,
    reason: fn(&JobEvent) -> Result<String, StartExecutionError>,
) -> Result<PreStartHookTerminalDispatch, StartExecutionError>
where
    P: ProcessController + ?Sized,
{
    context
        .start_store
        .remove_pre_start_sequence(sequence.operation_id);
    let service_cgroup_id = pre_start_hook_deadline(&sequence, job_event.job_id).service_cgroup_id;
    context
        .controller
        .kill_cgroup(&service_cgroup_id)
        .map_err(StartExecutionError::Boundary)?;
    let failure = apply_start_failure(
        &mut *context.services,
        &mut *context.operations,
        &mut *context.graph,
        StartFailureRequest {
            service: sequence.service,
            operation_id: sequence.operation_id,
            failed_at_ns,
            failure_cause: TransitionCause::PreHookFailure,
            reason: reason(&job_event)?,
        },
    )
    .map_err(StartExecutionError::StartFailure)?;

    Ok(PreStartHookTerminalDispatch {
        job_event,
        next_job_event: None,
        operation_events: failure.operation_events,
        service_transitions: failure.service_transitions,
        graph_events: failure.graph_events,
        killed_cgroup_id: Some(service_cgroup_id),
    })
}

fn create_next_hook<P>(
    context: &mut PreStartTerminalContext<'_, P>,
    job_event: JobEvent,
    mut sequence: PreStartHookSequence,
    command: Vec<String>,
    created_at_ns: u64,
) -> Result<PreStartHookTerminalDispatch, StartExecutionError>
where
    P: ProcessController + ?Sized,
{
    let job_id = context
        .job_ids
        .allocate_batch(1, created_at_ns)
        .map(|ids| ids[0])
        .map_err(StartExecutionError::JobIdAllocation)?;
    let job = JobRecord::new_pre_exec_hook(
        job_id,
        ServiceHookJobSpec {
            service: &sequence.definition,
            argv: command,
            hook_index: Some(sequence.next_index),
            resolved_identity: hook_execution_identity(&sequence.definition),
            token_summary: TokenSummary::requested_identity(hook_execution_identity(
                &sequence.definition,
            )),
            activation_generation: sequence.activation_generation,
            cgroup_generation: sequence.cgroup_generation,
            operation_id: sequence.operation_id,
            created_at_ns,
        },
    )
    .map_err(StartExecutionError::HookJob)?;
    let next_job_event = context
        .jobs
        .create_job(job)
        .map_err(StartExecutionError::JobStore)?;
    sequence.advance();
    context
        .start_store
        .record_pre_start_sequence(sequence.clone());
    context
        .start_store
        .record_pre_start_hook_deadline(pre_start_hook_deadline(&sequence, job_id));

    Ok(PreStartHookTerminalDispatch {
        job_event,
        next_job_event: Some(next_job_event),
        operation_events: Vec::new(),
        service_transitions: Vec::new(),
        graph_events: Vec::new(),
        killed_cgroup_id: None,
    })
}

fn create_main_after_hooks<P>(
    context: &mut PreStartTerminalContext<'_, P>,
    job_event: JobEvent,
    sequence: PreStartHookSequence,
    created_at_ns: u64,
) -> Result<PreStartHookTerminalDispatch, StartExecutionError>
where
    P: ProcessController + ?Sized,
{
    context
        .start_store
        .remove_pre_start_sequence(sequence.operation_id);
    let hooks_cgroup_id = pre_start_hook_deadline(&sequence, job_event.job_id).hooks_cgroup_id;
    context
        .controller
        .kill_cgroup(&hooks_cgroup_id)
        .map_err(StartExecutionError::Boundary)?;
    let readiness_deadline = readiness_deadline(&sequence);
    let main_job = JobRecord::new_service_main(
        sequence.main_job_id,
        ServiceMainJobSpec {
            service: &sequence.definition,
            resolved_identity: sequence.resolved_identity,
            token_summary: sequence.token_summary,
            activation_generation: sequence.activation_generation,
            cgroup_generation: sequence.cgroup_generation,
            operation_id: sequence.operation_id,
            created_at_ns,
        },
    );
    let next_job_event = context
        .jobs
        .create_job(main_job)
        .map_err(StartExecutionError::JobStore)?;
    if let Some(deadline) = readiness_deadline {
        context.start_store.record_readiness_deadline(deadline);
    }

    Ok(PreStartHookTerminalDispatch {
        job_event,
        next_job_event: Some(next_job_event),
        operation_events: Vec::new(),
        service_transitions: Vec::new(),
        graph_events: Vec::new(),
        killed_cgroup_id: Some(hooks_cgroup_id),
    })
}
