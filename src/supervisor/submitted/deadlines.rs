//! The deadlines peinit holds against submitted jobs: `timeout`,
//! `readiness_timeout`, the kill after a stop's grace, the post-kill check,
//! and a busy cgroup's retry.

use crate::boundary::ProcessController;
use crate::ids::JobId;
use crate::job::JobState;
use crate::submitted::{
    DEFAULT_SUBMITTED_JOB_RETENTION_NS, SubmittedJobCause, SubmittedJobDeadline,
    SubmittedJobDeadlineKind,
};

use super::stop::{SubmittedStopOutcome, begin_submitted_stop};
use crate::supervisor::dispatch::SupervisorSubmittedDeadlineDispatch;
use crate::supervisor::lifecycle_deadline_timer::{
    SupervisorLifecycleDeadline, SupervisorLifecycleDeadlineKind,
};
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub fn next_submitted_job_deadline(&self) -> Option<SupervisorLifecycleDeadline> {
        self.submitted
            .next_deadline(|job_id| {
                self.jobs
                    .get(job_id)
                    .and_then(|record| record.started_at_ns)
            })
            .map(SupervisorLifecycleDeadline::from)
    }

    pub(in crate::supervisor) fn due_submitted_job_deadlines(
        &self,
        now_ns: u64,
    ) -> Vec<SubmittedJobDeadline> {
        self.submitted.due_deadlines(now_ns, |job_id| {
            self.jobs
                .get(job_id)
                .and_then(|record| record.started_at_ns)
        })
    }

    /// Act on one due deadline. Returns `None` when there was nothing left
    /// to do — the job ended between the deadline being read and acted on.
    pub(in crate::supervisor) fn process_due_submitted_job_deadline<P>(
        &mut self,
        job_id: JobId,
        kind: SubmittedJobDeadlineKind,
        controller: &mut P,
        now_ns: u64,
    ) -> Result<Option<SupervisorSubmittedDeadlineDispatch>, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let post_kill_timeout_secs = self.settings.shutdown.post_kill_timeout_secs;
        let mut work = SupervisorWork::from_supervisor(self);
        let dispatch = process_submitted_deadline(
            &mut work,
            controller,
            job_id,
            kind,
            now_ns,
            post_kill_timeout_secs,
        )?;
        work.commit(self);
        Ok(dispatch)
    }
}

pub(in crate::supervisor) fn process_submitted_deadline<P>(
    work: &mut SupervisorWork,
    controller: &mut P,
    job_id: JobId,
    kind: SubmittedJobDeadlineKind,
    now_ns: u64,
    post_kill_timeout_secs: u64,
) -> Result<Option<SupervisorSubmittedDeadlineDispatch>, SupervisorError>
where
    P: ProcessController + ?Sized,
{
    match kind {
        SubmittedJobDeadlineKind::Timeout | SubmittedJobDeadlineKind::ReadinessTimeout => {
            let cause = match kind {
                SubmittedJobDeadlineKind::Timeout => SubmittedJobCause::Timeout,
                _ => SubmittedJobCause::ReadinessTimeout,
            };
            match begin_submitted_stop(
                work,
                controller,
                job_id,
                cause,
                now_ns,
                post_kill_timeout_secs,
            )? {
                SubmittedStopOutcome::Stopping(stop) => {
                    Ok(Some(SupervisorSubmittedDeadlineDispatch::Stop(stop)))
                }
                SubmittedStopOutcome::Unchanged | SubmittedStopOutcome::CancelledBeforeStart(_) => {
                    Ok(None)
                }
            }
        }
        SubmittedJobDeadlineKind::StopKill => {
            let Some(cgroup_id) = work
                .submitted
                .get(job_id)
                .map(|entry| entry.cgroup_id.clone())
            else {
                return Ok(None);
            };
            controller
                .kill_cgroup(&cgroup_id)
                .map_err(SupervisorError::ProcessControl)?;
            work.submitted
                .record_kill(job_id, now_ns, post_kill_timeout_secs)
                .map_err(SupervisorError::Submitted)?;
            Ok(Some(SupervisorSubmittedDeadlineDispatch::Killed { job_id }))
        }
        SubmittedJobDeadlineKind::PostKill => {
            let Some(cgroup_id) = work
                .submitted
                .get(job_id)
                .map(|entry| entry.cgroup_id.clone())
            else {
                return Ok(None);
            };
            let still_running = work
                .jobs
                .get(job_id)
                .is_some_and(|record| record.state == JobState::Running);
            if !still_running {
                return Ok(None);
            }
            if controller
                .cgroup_populated(&cgroup_id)
                .map_err(SupervisorError::ProcessControl)?
            {
                if let Some(entry) = work.submitted.get_mut(job_id) {
                    entry.cause = Some(SubmittedJobCause::ProcessUnkillable);
                    super::terminal::close_entry_descriptors(entry);
                }
                let job_event = work
                    .jobs
                    .abandon_job(job_id, now_ns, "process survived SIGKILL")
                    .map_err(SupervisorError::JobStore)?;
                work.submitted
                    .record_terminal(&job_event, DEFAULT_SUBMITTED_JOB_RETENTION_NS)
                    .map_err(SupervisorError::Submitted)?;
                Ok(Some(SupervisorSubmittedDeadlineDispatch::Abandoned {
                    job_event: Box::new(job_event),
                    cgroup_id,
                }))
            } else {
                // Empty but not yet reaped: the exit is on its way. Look again
                // after another grace rather than spinning.
                work.submitted
                    .record_kill(job_id, now_ns, post_kill_timeout_secs)
                    .map_err(SupervisorError::Submitted)?;
                Ok(None)
            }
        }
        SubmittedJobDeadlineKind::CgroupCleanup => {
            let Some(cgroup_id) = work
                .submitted
                .get(job_id)
                .map(|entry| entry.cgroup_id.clone())
            else {
                return Ok(None);
            };
            let leaked = matches!(
                controller
                    .remove_cgroup(&cgroup_id)
                    .map_err(SupervisorError::ProcessControl)?,
                crate::boundary::CgroupRemoveOutcome::Busy
            );
            if let Some(entry) = work.submitted.get_mut(job_id) {
                entry.cgroup_cleanup_due_at_ns = None;
            }
            Ok(Some(SupervisorSubmittedDeadlineDispatch::CgroupCleanup {
                job_id,
                leaked,
            }))
        }
    }
}

impl From<SubmittedJobDeadline> for SupervisorLifecycleDeadline {
    fn from(deadline: SubmittedJobDeadline) -> Self {
        Self {
            due_at_ns: deadline.due_at_ns,
            kind: SupervisorLifecycleDeadlineKind::SubmittedJob {
                job_id: deadline.job_id,
                kind: deadline.kind,
            },
        }
    }
}
