use crate::job::{JobExit, JobState, JobStoreError};
use crate::supervisor::dispatch::SupervisorHealthCheckTerminalDispatch;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::super::HealthCheckError;
use super::terminal::apply_health_check_terminal_in_work;

pub(in crate::supervisor) fn fail_created_health_check_in_work(
    work: &mut SupervisorWork,
    job_id: crate::ids::JobId,
    failed_at_ns: u64,
    reason: String,
) -> Result<SupervisorHealthCheckTerminalDispatch, SupervisorError> {
    let cgroup_id = work
        .jobs
        .get(job_id)
        .ok_or(JobStoreError::UnknownJob { id: job_id })
        .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?
        .cgroup_id
        .clone();
    let job_event = work
        .jobs
        .fail_job_before_start(job_id, failed_at_ns, reason)
        .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?;
    apply_health_check_terminal_in_work(work, job_event, cgroup_id, failed_at_ns)
}

/// End a health-check job that never ran, without counting it against the
/// service.
///
/// The job is failed so the invocation is closed and the next interval is
/// scheduled normally, but no health failure is recorded and nothing is
/// escalated. The distinction is between "the probe ran and said the service
/// is unhealthy" and "the probe could not be run" — only the first says
/// anything about the service (PEI-367).
pub(in crate::supervisor) fn fail_launched_health_check_in_work(
    work: &mut SupervisorWork,
    job_id: crate::ids::JobId,
    failed_at_ns: u64,
    reason: String,
) -> Result<SupervisorHealthCheckTerminalDispatch, SupervisorError> {
    let cgroup_id = work
        .jobs
        .get(job_id)
        .ok_or(JobStoreError::UnknownJob { id: job_id })
        .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?
        .cgroup_id
        .clone();
    let job_event = work
        .jobs
        .fail_job_before_start(job_id, failed_at_ns, reason)
        .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?;
    work.health.remove_invocation(job_id);
    Ok(
        crate::supervisor::dispatch::SupervisorHealthCheckTerminalDispatch {
            job_event,
            service_job_event: None,
            outcome: crate::supervisor::dispatch::SupervisorHealthCheckOutcome::NotLaunched,
            service_transitions: Vec::new(),
            killed_cgroup_id: cgroup_id,
            critical_reboot: None,
        },
    )
}

pub(in crate::supervisor) fn fail_timed_out_health_check_in_work(
    work: &mut SupervisorWork,
    job_id: crate::ids::JobId,
    now_ns: u64,
) -> Result<(SupervisorHealthCheckTerminalDispatch, String), SupervisorError> {
    let job = work
        .jobs
        .get(job_id)
        .cloned()
        .ok_or(JobStoreError::UnknownJob { id: job_id })
        .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?;
    let cgroup_id = job.cgroup_id.clone();
    let job_event = match job.state {
        JobState::Created => work
            .jobs
            .fail_job_before_start(job_id, now_ns, "health check timed out before launch")
            .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?,
        JobState::Running => work
            .jobs
            .fail_running_job(
                job_id,
                now_ns,
                Some(JobExit::Signal(9)),
                "health check timed out",
            )
            .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?,
        _ => work
            .jobs
            .fail_running_job(
                job_id,
                now_ns,
                Some(JobExit::Signal(9)),
                "health check timed out",
            )
            .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?,
    };
    let terminal = apply_health_check_terminal_in_work(work, job_event, cgroup_id.clone(), now_ns)?;
    Ok((terminal, cgroup_id))
}
