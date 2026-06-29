use crate::ids::{JobId, JobIdAllocator, OperationId};
use crate::job::{JobEvent, JobRecord, JobStore, ServiceMainJobSpec, service_cgroup_root_path};
use crate::security::TokenSummary;
use crate::service::ServiceActivationSnapshot;

use super::deadline::requires_notify_readiness;
use super::initial_pre::{create_initial_pre_start_hook, parse_pre_start_hooks};
use super::model::{StartExecutionError, StartExecutionJobKind};
use super::store::{ReadinessDeadline, StartExecutionStore};

pub(super) struct InitialStartJob {
    pub job_id: JobId,
    pub job_event: JobEvent,
    pub job_kind: StartExecutionJobKind,
}

pub(super) struct InitialStartJobRequest<'a> {
    pub service: &'a str,
    pub operation_id: OperationId,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub started_at_ns: u64,
    pub operation_deadline_ns: u64,
    pub activation: &'a ServiceActivationSnapshot,
    pub main_job_id: JobId,
}

pub(super) fn create_initial_start_job(
    jobs: &mut JobStore,
    job_ids: &mut JobIdAllocator,
    start_store: &mut StartExecutionStore,
    request: InitialStartJobRequest<'_>,
) -> Result<InitialStartJob, StartExecutionError> {
    let commands = parse_pre_start_hooks(request.activation)?;
    if commands.is_empty() {
        return create_initial_main_job(jobs, start_store, request);
    }
    create_initial_pre_start_hook(jobs, job_ids, start_store, request, commands)
}

fn create_initial_main_job(
    jobs: &mut JobStore,
    start_store: &mut StartExecutionStore,
    request: InitialStartJobRequest<'_>,
) -> Result<InitialStartJob, StartExecutionError> {
    let job_id = request.main_job_id;
    let job = service_main_job(&request, request.activation, job_id);
    let job_event = jobs
        .create_job(job)
        .map_err(StartExecutionError::JobStore)?;
    record_readiness_deadline(start_store, &request, job_id);
    Ok(InitialStartJob {
        job_id,
        job_event,
        job_kind: StartExecutionJobKind::ServiceMain,
    })
}

fn service_main_job(
    request: &InitialStartJobRequest<'_>,
    activation: &ServiceActivationSnapshot,
    job_id: JobId,
) -> JobRecord {
    JobRecord::new_service_main(
        job_id,
        ServiceMainJobSpec {
            service: &activation.definition,
            resolved_identity: request.resolved_identity.clone(),
            token_summary: request.token_summary.clone(),
            activation_generation: activation.activation_generation,
            cgroup_generation: activation.cgroup_generation,
            operation_id: request.operation_id,
            created_at_ns: request.started_at_ns,
        },
    )
}

fn record_readiness_deadline(
    start_store: &mut StartExecutionStore,
    request: &InitialStartJobRequest<'_>,
    job_id: JobId,
) {
    if !requires_notify_readiness(&request.activation.definition) {
        return;
    }
    start_store.record_readiness_deadline(ReadinessDeadline {
        operation_id: request.operation_id,
        job_id,
        service: request.service.to_string(),
        service_cgroup_id: service_cgroup_root_path(
            request.service,
            request.activation.cgroup_generation,
        ),
        due_at_ns: request.operation_deadline_ns,
    });
}
