use crate::ids::JobId;
use crate::job::{JobEvent, JobExit, JobState, JobStore};

use crate::execution::start::StartExecutionError;

pub(super) fn fail_timed_out_job(
    jobs: &mut JobStore,
    job_id: JobId,
    now_ns: u64,
    before_launch_reason: &'static str,
    running_reason: &'static str,
) -> Result<JobEvent, StartExecutionError> {
    let state = jobs
        .get(job_id)
        .ok_or(crate::job::JobStoreError::UnknownJob { id: job_id })
        .map_err(StartExecutionError::JobStore)?
        .state;
    match state {
        JobState::Created => jobs
            .fail_job_before_start(job_id, now_ns, before_launch_reason)
            .map_err(StartExecutionError::JobStore),
        JobState::Running => jobs
            .fail_running_job(job_id, now_ns, Some(JobExit::Signal(9)), running_reason)
            .map_err(StartExecutionError::JobStore),
        _ => jobs
            .fail_running_job(job_id, now_ns, Some(JobExit::Signal(9)), running_reason)
            .map_err(StartExecutionError::JobStore),
    }
}
