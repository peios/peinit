use crate::boundary::ProcessTarget;
use crate::job::{JobState, JobStore};

use super::model::ControlExecutionError;

pub(super) fn process_target(
    jobs: &JobStore,
    service: &str,
) -> Result<ProcessTarget, ControlExecutionError> {
    let job_id = jobs.current_service_main_job(service).ok_or_else(|| {
        ControlExecutionError::MissingCurrentMainJob {
            service: service.to_string(),
        }
    })?;
    let job = jobs
        .get(job_id)
        .ok_or(ControlExecutionError::MissingJobRecord { job_id })?;
    if job.state != JobState::Running {
        return Err(ControlExecutionError::JobNotRunning { job_id });
    }
    let (Some(pid), Some(pidfd)) = (job.pid, job.pidfd) else {
        return Err(ControlExecutionError::MissingProcessHandle { job_id });
    };

    Ok(ProcessTarget {
        service: service.to_string(),
        pid,
        pidfd,
        cgroup_id: job.cgroup_id.clone(),
    })
}
