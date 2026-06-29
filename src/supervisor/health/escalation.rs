use crate::boundary::ProcessController;
use crate::job::{JobExit, JobState, JobType, service_cgroup_root_path};
use crate::supervisor::cgroup_cleanup::{CgroupCleanupKind, record_cgroup_cleanup};
use crate::supervisor::dispatch::{
    SupervisorHealthCheckOutcome, SupervisorHealthCheckTerminalDispatch,
};
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

use super::HealthCheckError;

pub(in crate::supervisor) fn terminate_service_after_health_escalation<P>(
    work: &mut SupervisorWork,
    terminal: &mut SupervisorHealthCheckTerminalDispatch,
    service: &str,
    cgroup_generation: u64,
    controller: &mut P,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<(), SupervisorError>
where
    P: ProcessController + ?Sized,
{
    if !health_outcome_requires_service_termination(&terminal.outcome) {
        return Ok(());
    }

    let service_cgroup_id = service_cgroup_root_path(service, cgroup_generation);
    controller
        .kill_cgroup(&service_cgroup_id)
        .map_err(|error| SupervisorError::Health(HealthCheckError::Boundary(error)))?;
    let job_id = work.jobs.current_service_main_job(service).ok_or_else(|| {
        SupervisorError::Health(HealthCheckError::MissingMainJob {
            service: service.to_string(),
        })
    })?;
    let job = work
        .jobs
        .get(job_id)
        .ok_or(SupervisorError::Health(HealthCheckError::JobStore(
            crate::job::JobStoreError::UnknownJob { id: job_id },
        )))?;
    if job.job_type != JobType::ServiceMain || job.state != JobState::Running {
        return Err(SupervisorError::Health(HealthCheckError::MissingMainJob {
            service: service.to_string(),
        }));
    }

    let job_event = work
        .jobs
        .fail_running_job(
            job_id,
            now_ns,
            Some(JobExit::Signal(libc::SIGKILL)),
            "health check failed",
        )
        .map_err(|error| SupervisorError::Health(HealthCheckError::JobStore(error)))?;
    terminal.service_job_event = Some(job_event);
    record_cgroup_cleanup(
        &mut work.cgroup_cleanup,
        service,
        service_cgroup_id,
        CgroupCleanupKind::ServiceTree,
        now_ns,
        post_kill_timeout_secs,
    );

    Ok(())
}

fn health_outcome_requires_service_termination(outcome: &SupervisorHealthCheckOutcome) -> bool {
    matches!(
        outcome,
        SupervisorHealthCheckOutcome::RestartScheduled { .. }
            | SupervisorHealthCheckOutcome::Failed { .. }
    )
}
