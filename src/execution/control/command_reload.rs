use crate::boundary::ProcessTarget;
use crate::job::{JobRecord, ServiceHookJobSpec};
use crate::security::TokenSummary;
use crate::service::ServiceDefinition;
use crate::service::runtime::{ServiceState, ServiceTransition, TransitionCause};

use super::model::{
    ControlExecutionContext, ControlExecutionDetail, ControlExecutionDispatch,
    ControlExecutionError, ControlOperationKind, ControlOperationRequest,
};
use super::store::ReloadCommandDeadline;

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub(super) fn begin_command_reload<P>(
    context: &mut ControlExecutionContext<'_, P>,
    request: ControlOperationRequest,
    target: ProcessTarget,
    definition: &ServiceDefinition,
    argv: Vec<String>,
) -> Result<ControlExecutionDispatch, ControlExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_jobs = context.jobs.clone();
    let mut next_job_ids = context.job_ids.clone();
    let mut next_store = context.control_store.clone();

    let job_id = next_job_ids
        .allocate_batch(1, request.observed_at_ns)
        .map(|ids| ids[0])
        .map_err(ControlExecutionError::JobIdAllocation)?;
    let runtime = next_services.runtime(&target.service).ok_or_else(|| {
        ControlExecutionError::ServiceTable(crate::service::ServiceTableError::UnknownService {
            service: target.service.clone(),
        })
    })?;
    let activation_generation = runtime.generation;
    let cgroup_generation = runtime.cgroup_generation;
    let operation_event = next_operations
        .start_operation(request.operation_id, request.observed_at_ns)
        .map_err(ControlExecutionError::OperationStore)?;
    let operation = next_operations
        .get(request.operation_id)
        .ok_or(
            crate::operation::store::OperationStoreError::UnknownOperation {
                id: request.operation_id,
            },
        )
        .map_err(ControlExecutionError::OperationStore)?
        .clone();
    let service_transition = next_services
        .transition_service(
            &target.service,
            ServiceTransition {
                to: ServiceState::Reloading,
                cause: TransitionCause::ExplicitReload,
            },
        )
        .map_err(ControlExecutionError::ServiceTable)?;
    let job = JobRecord::new_reload_hook(
        job_id,
        ServiceHookJobSpec {
            service: definition,
            argv,
            hook_index: None,
            resolved_identity: definition.identity.clone(),
            token_summary: TokenSummary::requested_identity(definition.identity.clone()),
            activation_generation,
            cgroup_generation,
            operation_id: request.operation_id,
            created_at_ns: request.observed_at_ns,
        },
    )
    .map_err(ControlExecutionError::HookJob)?;
    let job_cgroup_id = job.cgroup_id.clone();
    let job_event = next_jobs
        .create_job(job)
        .map_err(ControlExecutionError::JobStore)?;
    let deadline_ns = operation
        .created_at_ns
        .saturating_add(definition.start_timeout_secs.saturating_mul(NANOS_PER_SEC));
    next_store.record_reload_command_deadline(ReloadCommandDeadline {
        operation_id: request.operation_id,
        job_id,
        service: target.service.clone(),
        cgroup_id: job_cgroup_id,
        due_at_ns: deadline_ns,
    });

    *context.services = next_services;
    *context.operations = next_operations;
    *context.jobs = next_jobs;
    *context.job_ids = next_job_ids;
    *context.control_store = next_store;

    Ok(ControlExecutionDispatch {
        operation_id: request.operation_id,
        service: target.service.clone(),
        kind: ControlOperationKind::ReloadCommand,
        operation_event,
        service_transition: Some(service_transition),
        detail: ControlExecutionDetail::ReloadCommand {
            job_id,
            job_event: Box::new(job_event),
        },
        deadline_ns,
        cancelled_reload_jobs: Vec::new(),
    })
}
