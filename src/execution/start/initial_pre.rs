use crate::execution::command::parse_executable_command;
use crate::ids::{JobId, JobIdAllocator};
use crate::job::{JobRecord, JobStore, ServiceHookJobSpec, service_cgroup_root_path};
use crate::security::{TokenSummary, hook_execution_identity};
use crate::service::ServiceActivationSnapshot;

use super::initial::{InitialStartJob, InitialStartJobRequest};
use super::model::{StartExecutionError, StartExecutionJobKind};
use super::store::{PreStartHookDeadline, PreStartHookSequence, StartExecutionStore};

pub(super) fn create_initial_pre_start_hook(
    jobs: &mut JobStore,
    job_ids: &mut JobIdAllocator,
    start_store: &mut StartExecutionStore,
    request: InitialStartJobRequest<'_>,
    commands: Vec<Vec<String>>,
) -> Result<InitialStartJob, StartExecutionError> {
    let hook_job_id = job_ids
        .allocate_batch(1, request.started_at_ns)
        .map(|ids| ids[0])
        .map_err(StartExecutionError::JobIdAllocation)?;
    let deadline_ns = request.operation_deadline_ns;
    let Some(first_command) = commands.first().cloned() else {
        return Err(StartExecutionError::EmptyExecStartPreSequence {
            service: request.service.to_string(),
        });
    };
    let job = pre_start_hook_job(&request, hook_job_id, first_command)?;
    let hooks_cgroup_id = job.cgroup_id.clone();
    let job_event = jobs
        .create_job(job)
        .map_err(StartExecutionError::JobStore)?;
    start_store.record_pre_start_sequence(PreStartHookSequence {
        service: request.service.to_string(),
        operation_id: request.operation_id,
        main_job_id: request.main_job_id,
        definition: request.activation.definition.clone(),
        resolved_identity: request.resolved_identity.clone(),
        token_summary: request.token_summary.clone(),
        activation_generation: request.activation.activation_generation,
        cgroup_generation: request.activation.cgroup_generation,
        commands,
        next_index: 1,
        deadline_ns,
    });
    start_store.record_pre_start_hook_deadline(PreStartHookDeadline {
        operation_id: request.operation_id,
        job_id: hook_job_id,
        service: request.service.to_string(),
        hooks_cgroup_id,
        service_cgroup_id: service_cgroup_root_path(
            request.service,
            request.activation.cgroup_generation,
        ),
        due_at_ns: deadline_ns,
    });

    Ok(InitialStartJob {
        job_id: hook_job_id,
        job_event,
        job_kind: StartExecutionJobKind::PreStartHook {
            main_job_id: request.main_job_id,
        },
    })
}

pub(super) fn parse_pre_start_hooks(
    activation: &ServiceActivationSnapshot,
) -> Result<Vec<Vec<String>>, StartExecutionError> {
    activation
        .definition
        .exec_start_pre
        .iter()
        .enumerate()
        .map(|(command_index, command)| {
            parse_executable_command(command).map_err(|source| {
                StartExecutionError::InvalidExecStartPreCommand {
                    service: activation.service.clone(),
                    command_index,
                    command: command.clone(),
                    source,
                }
            })
        })
        .collect()
}

fn pre_start_hook_job(
    request: &InitialStartJobRequest<'_>,
    job_id: JobId,
    argv: Vec<String>,
) -> Result<JobRecord, StartExecutionError> {
    let resolved_identity = hook_execution_identity(&request.activation.definition);
    JobRecord::new_pre_exec_hook(
        job_id,
        ServiceHookJobSpec {
            service: &request.activation.definition,
            argv,
            hook_index: Some(0),
            token_summary: TokenSummary::requested_identity(resolved_identity.clone()),
            resolved_identity,
            activation_generation: request.activation.activation_generation,
            cgroup_generation: request.activation.cgroup_generation,
            operation_id: request.operation_id,
            created_at_ns: request.started_at_ns,
        },
    )
    .map_err(StartExecutionError::HookJob)
}
