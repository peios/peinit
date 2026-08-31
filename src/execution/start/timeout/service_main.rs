use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::ids::JobId;
use crate::job::{JobState, service_cgroup_root_path};
use crate::operation::operation_timeout_result;
use crate::service::runtime::TransitionCause;

use crate::execution::start::{
    ServiceMainStartTimeoutDispatch, StartExecutionContext, StartExecutionError,
};

use super::job::fail_timed_out_job;

pub fn timeout_service_main_start<P>(
    context: &mut StartExecutionContext<'_, P>,
    job_id: JobId,
    now_ns: u64,
) -> Result<ServiceMainStartTimeoutDispatch, StartExecutionError>
where
    P: crate::boundary::ProcessController + ?Sized,
{
    let mut next_services = context.services.clone();
    let mut next_operations = context.operations.clone();
    let mut next_graph = context.graph.clone();
    let mut next_jobs = context.jobs.clone();

    let job = next_jobs
        .get(job_id)
        .cloned()
        .ok_or(crate::job::JobStoreError::UnknownJob { id: job_id })
        .map_err(StartExecutionError::JobStore)?;
    let service = job
        .service
        .clone()
        .ok_or(StartExecutionError::MissingService { job_id })?;
    let operation_id = job
        .operation_id
        .ok_or_else(|| StartExecutionError::MissingOperation {
            job_id,
            service: service.clone(),
        })?;
    let killed_cgroup_id = if job.state == JobState::Running {
        let cgroup_id = service_cgroup_root_path(&service, job.cgroup_generation);
        context
            .controller
            .kill_cgroup(&cgroup_id)
            .map_err(StartExecutionError::Boundary)?;
        Some(cgroup_id)
    } else {
        None
    };
    let job_event = fail_timed_out_job(
        &mut next_jobs,
        job_id,
        now_ns,
        "service main timed out before launch",
        "service main timed out",
    )?;
    let failure = apply_start_failure(
        &mut next_services,
        &mut next_operations,
        &mut next_graph,
        StartFailureRequest {
            service,
            operation_id,
            failed_at_ns: now_ns,
            failure_cause: TransitionCause::ReadinessTimeout,
            reason: operation_timeout_result("start operation timed out"),
            exit_code: None,
        },
    )
    .map_err(StartExecutionError::StartFailure)?;

    *context.services = next_services;
    *context.operations = next_operations;
    *context.graph = next_graph;
    *context.jobs = next_jobs;

    Ok(ServiceMainStartTimeoutDispatch {
        job_event,
        operation_events: failure.operation_events,
        service_transitions: failure.service_transitions,
        graph_events: failure.graph_events,
        killed_cgroup_id,
        timed_out_at_ns: now_ns,
    })
}
