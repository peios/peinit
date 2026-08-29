use crate::boundary::ProcessController;
use crate::ids::JobId;
use crate::submitted::{DEFAULT_SUBMITTED_JOB_RETENTION_NS, SubmittedJobCause};

use super::terminal::{close_entry_descriptors, release_job_cgroup};
use crate::supervisor::dispatch::SupervisorSubmittedLaunchFailureDispatch;
use crate::supervisor::state::SupervisorError;
use crate::supervisor::work::SupervisorWork;

/// A submitted job that never ran: parent-side setup failed, or the child
/// failed between fork and exec. The record goes straight to `failed`, the
/// entry keeps the outcome, and everything the entry still held is closed.
pub(in crate::supervisor) fn apply_submitted_setup_failure<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    job_id: JobId,
    failed_at_ns: u64,
    cause: SubmittedJobCause,
    failure_cause: String,
    post_kill_timeout_secs: u64,
) -> Result<SupervisorSubmittedLaunchFailureDispatch, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    if let Some(entry) = work.submitted.get_mut(job_id) {
        entry.cause = Some(cause);
        close_entry_descriptors(entry);
    }
    let job_event = work
        .jobs
        .fail_job_before_start(job_id, failed_at_ns, failure_cause)
        .map_err(SupervisorError::JobStore)?;
    let entry = work
        .submitted
        .record_terminal(&job_event, DEFAULT_SUBMITTED_JOB_RETENTION_NS)
        .map_err(SupervisorError::Submitted)?;
    let cgroup_id = entry.cgroup_id.clone();
    release_job_cgroup(
        work,
        controller,
        job_id,
        &cgroup_id,
        failed_at_ns,
        post_kill_timeout_secs,
    )?;
    Ok(SupervisorSubmittedLaunchFailureDispatch { job_event, cause })
}
