use crate::execution::failure::{StartFailureRequest, apply_start_failure};
use crate::execution::launch::LaunchCreatedJobError;
use crate::execution::start::StartExecutionError;
use crate::ids::JobId;
use crate::job::service_cgroup_root_path;
use crate::service::runtime::TransitionCause;

use super::classify::classify_launch_failure;
use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::dispatch::SupervisorLaunchFailureDispatch;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

pub(in crate::supervisor) fn apply_service_launch_failure(
    work: &mut SupervisorWork,
    job_id: JobId,
    failed_at_ns: u64,
    error: LaunchCreatedJobError,
    max_parallel_starts: u32,
    post_kill_timeout_secs: u64,
) -> Result<Option<SupervisorLaunchFailureDispatch>, SupervisorError> {
    let Some(classification) = classify_launch_failure(&error) else {
        return Ok(None);
    };
    let job = work
        .jobs
        .get(job_id)
        .cloned()
        .ok_or(crate::job::JobStoreError::UnknownJob { id: job_id })
        .map_err(SupervisorError::JobStore)?;
    let job_event = work
        .jobs
        .fail_job_before_start(job_id, failed_at_ns, classification.reason.clone())
        .map_err(SupervisorError::JobStore)?;
    let Some(service) = job_event.service.clone() else {
        return Err(SupervisorError::Launch(
            LaunchCreatedJobError::ServiceMainJobMissingService { job_id },
        ));
    };
    let operation_id = job_event.operation_id.ok_or_else(|| {
        SupervisorError::Start(StartExecutionError::MissingOperation {
            job_id,
            service: service.clone(),
        })
    })?;
    let failure_cause = classification.cause;

    work.start.remove_readiness_deadline(operation_id);
    let failure = apply_start_failure(
        &mut work.services,
        &mut work.operations,
        &mut work.graph,
        StartFailureRequest {
            service: service.clone(),
            operation_id,
            failed_at_ns,
            failure_cause,
            reason: classification.reason,
            exit_code: None,
        },
    )
    .map_err(|error| SupervisorError::Start(StartExecutionError::StartFailure(error)))?;
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
    record_failed_service_launch_cleanup(
        work,
        &service,
        &job_event.service,
        job.cgroup_generation,
        failed_at_ns,
        failure_cause,
        post_kill_timeout_secs,
    );

    Ok(Some(SupervisorLaunchFailureDispatch {
        job_event,
        failure,
        start_dispatches,
    }))
}

fn record_failed_service_launch_cleanup(
    work: &mut SupervisorWork,
    service: &str,
    event_service: &Option<String>,
    cgroup_generation: u64,
    failed_at_ns: u64,
    failure_cause: TransitionCause,
    post_kill_timeout_secs: u64,
) {
    record_cgroup_cleanup(
        &mut work.cgroup_cleanup,
        event_service.as_deref().unwrap_or(service),
        service_cgroup_root_path(service, cgroup_generation),
        CgroupCleanupKind::ServiceTree,
        failed_at_ns,
        cgroup_cleanup_delay_secs(failure_cause, post_kill_timeout_secs),
    );
}

fn cgroup_cleanup_delay_secs(cause: TransitionCause, post_kill_timeout_secs: u64) -> u64 {
    match cause {
        TransitionCause::ParentSetupFailure => 0,
        _ => post_kill_timeout_secs,
    }
}
