use crate::boundary::ProcessTarget;
use crate::job::{JobState, JobStore};
use crate::shutdown::ShutdownError;

pub(super) fn process_target(
    jobs: &JobStore,
    service: &str,
) -> Result<ProcessTarget, ShutdownError> {
    let job_id = jobs.current_service_main_job(service).ok_or_else(|| {
        ShutdownError::MissingRunningService {
            service: service.to_string(),
        }
    })?;
    let job = jobs
        .get(job_id)
        .ok_or(ShutdownError::MissingJobRecord { job_id })?;
    if job.state != JobState::Running {
        return Err(ShutdownError::JobNotRunning { job_id });
    }
    let (Some(pid), Some(pidfd)) = (job.pid, job.pidfd) else {
        return Err(ShutdownError::MissingProcessHandle { job_id });
    };
    Ok(ProcessTarget {
        service: service.to_string(),
        pid,
        pidfd,
        cgroup_id: job.cgroup_id.clone(),
    })
}
