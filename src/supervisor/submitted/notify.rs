//! A submitted job speaking the notification channel (PSPU §4.18, §4.19):
//! authenticated as *which supervised job this is*, then `READY`, `STATUS`,
//! `PROGRESS`, `PROGRESS_UNIT` and `STOPPING` applied to its entry.

use crate::boundary::ProcessController;
use crate::execution::notify::NotifyApplyError;
use crate::ids::JobId;
use crate::job::JobState;
use crate::notify::{NotifyField, NotifyMessage};
use crate::submitted::{JobReadiness, SubmittedNotifyField, parse_progress, parse_progress_unit};

use crate::supervisor::dispatch::{SupervisorNotifyDispatch, SupervisorSubmittedNotifyDispatch};
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::work::SupervisorWork;

/// The minimum spacing between `job.status` events for one job (PSPU §4.A).
pub const JOB_STATUS_EVENT_INTERVAL_NS: u64 = 1_000_000_000;

/// What a notification datagram did: a service's, or a submitted job's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorNotifyOutcome {
    Service(Box<SupervisorNotifyDispatch>),
    SubmittedJob(SupervisorSubmittedNotifyDispatch),
}

impl SupervisorNotifyOutcome {
    pub fn into_service(self) -> Option<SupervisorNotifyDispatch> {
        match self {
            Self::Service(dispatch) => Some(*dispatch),
            Self::SubmittedJob(_) => None,
        }
    }
}

impl Supervisor {
    /// §4.18 for a submitted job: the sender's PID is a live submitted job's,
    /// the job is running, and the fork-time handle still names that PID.
    /// There is no generation step — a submitted job has no replacement.
    pub(in crate::supervisor) fn authenticate_submitted_notify_sender<P>(
        &self,
        sender_pid: u32,
        controller: &mut P,
    ) -> Result<Option<JobId>, NotifyApplyError>
    where
        P: ProcessController + ?Sized,
    {
        for job_id in self.submitted.live_ids() {
            let Some(job) = self.jobs.get(job_id) else {
                continue;
            };
            if job.pid != Some(sender_pid) {
                continue;
            }
            if job.state != JobState::Running {
                return Err(NotifyApplyError::JobNotRunning {
                    job_id,
                    state: job.state,
                });
            }
            let Some(pidfd) = job.pidfd else {
                return Err(NotifyApplyError::MissingProcess { job_id });
            };
            if !controller
                .pidfd_matches_pid(pidfd, sender_pid)
                .map_err(|error| NotifyApplyError::ProcessVerification {
                    job_id,
                    pid: sender_pid,
                    pidfd,
                    message: format!("{error:?}"),
                })?
            {
                return Err(NotifyApplyError::PidfdMismatch {
                    job_id,
                    pid: sender_pid,
                    pidfd,
                });
            }
            return Ok(Some(job_id));
        }
        Ok(None)
    }

    pub(in crate::supervisor) fn apply_submitted_notify(
        &mut self,
        job_id: JobId,
        message: &NotifyMessage,
        observed_at_ns: u64,
    ) -> Result<SupervisorSubmittedNotifyDispatch, SupervisorError> {
        let mut work = SupervisorWork::from_supervisor(self);
        let dispatch = apply_submitted_notify_fields(&mut work, job_id, message, observed_at_ns)?;
        work.commit(self);
        Ok(dispatch)
    }
}

fn apply_submitted_notify_fields(
    work: &mut SupervisorWork,
    job_id: JobId,
    message: &NotifyMessage,
    observed_at_ns: u64,
) -> Result<SupervisorSubmittedNotifyDispatch, SupervisorError> {
    let entry = work
        .submitted
        .get_mut(job_id)
        .ok_or(SupervisorError::Submitted(
            crate::submitted::SubmittedJobStoreError::UnknownJob { id: job_id },
        ))?;
    let mut applied = Vec::new();
    for field in &message.fields {
        match field {
            NotifyField::Ready => {
                if entry.definition.readiness == JobReadiness::Notify && entry.ready == Some(false)
                {
                    entry.ready = Some(true);
                    applied.push(SubmittedNotifyField::Ready);
                }
            }
            NotifyField::Status(text) => {
                entry.status_text = Some(text.clone());
                applied.push(SubmittedNotifyField::Status { text: text.clone() });
            }
            // A submitted job is not a service and nothing can declare a
            // dependency on one, so its level is recorded and no more. The
            // field is accepted rather than refused because a program that
            // can run as either should not have to know which it is.
            NotifyField::Level(value) => {
                applied.push(SubmittedNotifyField::Level { value: value.clone() });
            }
            NotifyField::Progress(value) => {
                // An unexpected value is ignored, never repaired (§4.17).
                if let Ok(progress) = parse_progress(value) {
                    entry.progress = Some(progress);
                    applied.push(SubmittedNotifyField::Progress { progress });
                }
            }
            NotifyField::ProgressUnit(value) => {
                if let Some(unit) = parse_progress_unit(value) {
                    entry.progress_unit = Some(unit);
                    applied.push(SubmittedNotifyField::ProgressUnit { unit });
                }
            }
            NotifyField::Stopping => {
                entry.stopping_acknowledged = true;
            }
            // Transitions a job is never in, keepalives it has no watchdog
            // for, and a store it does not have: ignored, as §4.19 says.
            NotifyField::Reloading
            | NotifyField::Errno(_)
            | NotifyField::ExitStatus(_)
            | NotifyField::Watchdog
            | NotifyField::WatchdogUsec(_)
            | NotifyField::ExtendTimeoutUsec(_)
            | NotifyField::FdStore
            | NotifyField::FdName(_)
            | NotifyField::FdStoreRemove
            | NotifyField::FdPoll(_) => {}
        }
    }
    let reports = applied.iter().any(|field| {
        matches!(
            field,
            SubmittedNotifyField::Status { .. }
                | SubmittedNotifyField::Progress { .. }
                | SubmittedNotifyField::ProgressUnit { .. }
        )
    });
    let status_event_due = reports
        && entry
            .last_status_event_ns
            .is_none_or(|last| observed_at_ns.saturating_sub(last) >= JOB_STATUS_EVENT_INTERVAL_NS);
    if status_event_due {
        entry.last_status_event_ns = Some(observed_at_ns);
    }
    Ok(SupervisorSubmittedNotifyDispatch {
        job_id,
        submitter_sid: entry.submitter_sid.clone(),
        applied,
        status_event_due,
        status_text: entry.status_text.clone(),
        progress: entry.progress,
        progress_unit: entry.progress_unit,
    })
}
