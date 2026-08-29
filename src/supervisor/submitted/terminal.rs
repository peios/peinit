use crate::boundary::{CgroupRemoveOutcome, ChildExitStatus, ProcessController};
use crate::ids::JobId;
use crate::job::JobExit;
use crate::submitted::{DEFAULT_SUBMITTED_JOB_RETENTION_NS, SubmittedJobEntry};

use super::submit::close_fd;
use crate::supervisor::child_reap::signal_failure_cause;
use crate::supervisor::dispatch::SupervisedSubmittedTerminalDispatch;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    /// A submitted job's process was reaped. Success is the job's own
    /// `success_exit_codes`; the outcome is retained for the grace period;
    /// the cgroup is killed of anything left and removed.
    pub(in crate::supervisor) fn apply_submitted_reap<P>(
        &mut self,
        job_id: JobId,
        status: ChildExitStatus,
        ended_at_ns: u64,
        controller: &mut P,
    ) -> Result<SupervisedSubmittedTerminalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let post_kill_timeout_secs = self.settings.shutdown.post_kill_timeout_secs;
        let mut work = SupervisorWork::from_supervisor(self);
        let dispatch = apply_submitted_terminal(
            &mut work,
            controller,
            job_id,
            status,
            ended_at_ns,
            post_kill_timeout_secs,
        )?;
        work.commit(self);
        Ok(dispatch)
    }
}

pub(in crate::supervisor) fn apply_submitted_terminal<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    job_id: JobId,
    status: ChildExitStatus,
    ended_at_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<SupervisedSubmittedTerminalDispatch, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    let succeeded = match status {
        ChildExitStatus::Exited { code } => work
            .submitted
            .get(job_id)
            .is_some_and(|entry| entry.definition.is_success_exit_code(code)),
        ChildExitStatus::Signaled { .. } => false,
    };
    let job_event = match status {
        ChildExitStatus::Exited { code } if succeeded => work
            .jobs
            .complete_job(job_id, ended_at_ns, code)
            .map_err(SupervisorError::JobStore)?,
        ChildExitStatus::Exited { code } => work
            .jobs
            .fail_running_job(
                job_id,
                ended_at_ns,
                Some(JobExit::ExitCode(code)),
                format!("ProcessExit: code {code}"),
            )
            .map_err(SupervisorError::JobStore)?,
        ChildExitStatus::Signaled {
            signal,
            core_dumped,
        } => work
            .jobs
            .fail_running_job(
                job_id,
                ended_at_ns,
                Some(JobExit::Signal(signal)),
                signal_failure_cause(signal, core_dumped),
            )
            .map_err(SupervisorError::JobStore)?,
    };
    let (cause, cgroup_id) = {
        let entry = work
            .submitted
            .get_mut(job_id)
            .ok_or(SupervisorError::Submitted(
                crate::submitted::SubmittedJobStoreError::UnknownJob { id: job_id },
            ))?;
        close_entry_descriptors(entry);
        (entry.cause, entry.cgroup_id.clone())
    };
    let entry = work
        .submitted
        .record_terminal(&job_event, DEFAULT_SUBMITTED_JOB_RETENTION_NS)
        .map_err(SupervisorError::Submitted)?;
    let cause = entry.cause.or(cause);
    // Whatever the job left behind in its containment goes with it.
    controller
        .kill_cgroup(&cgroup_id)
        .map_err(SupervisorError::ProcessControl)?;
    let cgroup_busy = release_job_cgroup(
        work,
        controller,
        job_id,
        &cgroup_id,
        ended_at_ns,
        post_kill_timeout_secs,
    )?;
    Ok(SupervisedSubmittedTerminalDispatch {
        job_event,
        cause,
        cgroup_busy,
    })
}

/// Remove a terminal job's cgroup, scheduling one retry if it is busy.
/// Returns whether it was busy.
pub(super) fn release_job_cgroup<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    job_id: JobId,
    cgroup_id: &str,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<bool, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    let busy = matches!(
        controller
            .remove_cgroup(cgroup_id)
            .map_err(SupervisorError::ProcessControl)?,
        CgroupRemoveOutcome::Busy
    );
    if let Some(entry) = work.submitted.get_mut(job_id) {
        entry.cgroup_cleanup_due_at_ns = busy
            .then(|| now_ns.saturating_add(post_kill_timeout_secs.saturating_mul(1_000_000_000)));
    }
    Ok(busy)
}

/// Close every descriptor an entry still holds on the supervisor's behalf:
/// the prepared token, the attached descriptors, and an output sink the
/// runtime never adopted. Idempotent.
pub(in crate::supervisor) fn close_entry_descriptors(entry: &mut SubmittedJobEntry) {
    if let Some(fd) = entry.prepared_token_fd.take() {
        close_fd(fd);
    }
    for (_, fd) in std::mem::take(&mut entry.attached_descriptors) {
        close_fd(fd);
    }
    if let Some(fd) = entry.output_sink_fd.take() {
        close_fd(fd);
    }
}
