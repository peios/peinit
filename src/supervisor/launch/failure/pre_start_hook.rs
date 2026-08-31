use crate::boundary::ProcessController;
use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::execution::launch::LaunchCreatedJobError;
use crate::execution::start::StartExecutionError;
use crate::ids::JobId;
use crate::job::service_cgroup_root_path;
use crate::service::runtime::TransitionCause;

use super::classify::pre_start_hook_launch_failure_reason;
use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::dispatch::SupervisorStartHookLaunchFailureDispatch;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

pub(in crate::supervisor) fn apply_pre_start_hook_launch_failure<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    job_id: JobId,
    failed_at_ns: u64,
    error: LaunchCreatedJobError,
    max_parallel_starts: u32,
    post_kill_timeout_secs: u64,
) -> Result<Option<SupervisorStartHookLaunchFailureDispatch>, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    let Some(reason) = pre_start_hook_launch_failure_reason(&error) else {
        return Ok(None);
    };
    let job = work
        .jobs
        .get(job_id)
        .cloned()
        .ok_or(crate::job::JobStoreError::UnknownJob { id: job_id })
        .map_err(SupervisorError::JobStore)?;
    let service = job.service.clone().ok_or(SupervisorError::Start(
        StartExecutionError::MissingService { job_id },
    ))?;
    let operation_id = job.operation_id.ok_or_else(|| {
        SupervisorError::Start(StartExecutionError::MissingOperation {
            job_id,
            service: service.clone(),
        })
    })?;
    let killed_cgroup_id = service_cgroup_root_path(&service, job.cgroup_generation);

    let mut next_services = work.services.clone();
    let mut next_operations = work.operations.clone();
    let mut next_graph = work.graph.clone();
    let mut next_jobs = work.jobs.clone();
    let mut next_start = work.start.clone();

    next_start.remove_pre_start_hook_deadline(operation_id);
    next_start.remove_pre_start_sequence(operation_id);
    controller
        .kill_cgroup(&killed_cgroup_id)
        .map_err(StartExecutionError::Boundary)
        .map_err(SupervisorError::Start)?;
    let job_event = next_jobs
        .fail_job_before_start(job_id, failed_at_ns, reason.clone())
        .map_err(SupervisorError::JobStore)?;
    let failure = apply_start_failure(
        &mut next_services,
        &mut next_operations,
        &mut next_graph,
        StartFailureRequest {
            service: service.clone(),
            operation_id,
            failed_at_ns,
            failure_cause: TransitionCause::PreHookFailure,
            reason,
            exit_code: None,
        },
    )
    .map_err(|error| SupervisorError::Start(StartExecutionError::StartFailure(error)))?;

    work.services = next_services;
    work.operations = next_operations;
    work.graph = next_graph;
    work.jobs = next_jobs;
    work.start = next_start;
    record_failed_pre_start_hook_cleanup(
        work,
        job_event.service.as_deref().unwrap_or(service.as_str()),
        &killed_cgroup_id,
        failed_at_ns,
        post_kill_timeout_secs,
    );
    let mut start_dispatches = apply_relationship_reactions_after_transitions(
        work,
        &failure.service_transitions,
        failed_at_ns,
        max_parallel_starts,
    )?;
    start_dispatches.extend(work.release_after_graph_events(
        &failure.graph_events,
        max_parallel_starts,
        failed_at_ns,
    )?);

    Ok(Some(SupervisorStartHookLaunchFailureDispatch {
        job_event,
        failure,
        killed_cgroup_id,
        start_dispatches,
    }))
}

fn record_failed_pre_start_hook_cleanup(
    work: &mut SupervisorWork,
    service: &str,
    killed_cgroup_id: &str,
    failed_at_ns: u64,
    post_kill_timeout_secs: u64,
) {
    record_cgroup_cleanup(
        &mut work.cgroup_cleanup,
        service,
        format!("{killed_cgroup_id}/hooks"),
        CgroupCleanupKind::Hooks,
        failed_at_ns,
        post_kill_timeout_secs,
    );
    record_cgroup_cleanup(
        &mut work.cgroup_cleanup,
        service,
        killed_cgroup_id,
        CgroupCleanupKind::ServiceTree,
        failed_at_ns,
        post_kill_timeout_secs,
    );
}
