use std::collections::BTreeMap;

use crate::ids::JobId;
use crate::job::{JobEvent, JobEventDetail, JobState};

use super::model::{
    DEFAULT_SUBMITTED_JOB_RETENTION_NS, JobReadiness, SubmittedJobCause, SubmittedJobEntry,
    SubmittedJobOutcome, SubmittedJobStop, SubmittedJobStopPhase,
};

const NANOS_PER_SEC: u64 = 1_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmittedJobStoreError {
    DuplicateJobId { id: JobId },
    UnknownJob { id: JobId },
    AlreadyTerminal { id: JobId },
    NotTerminal { id: JobId },
}

/// What a submitted job's next deadline is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmittedJobDeadlineKind {
    /// `timeout` elapsed since the job started.
    Timeout,
    /// A `Notify` job has not sent `READY=1` within `readiness_timeout`.
    ReadinessTimeout,
    /// The termination signal's grace elapsed: kill the cgroup.
    StopKill,
    /// The post-kill grace elapsed: either the job was reaped, or it is
    /// unkillable.
    PostKill,
    /// A busy cgroup is retried.
    CgroupCleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedJobDeadline {
    pub job_id: JobId,
    pub kind: SubmittedJobDeadlineKind,
    pub due_at_ns: u64,
}

/// The `job-list` filters (PSPU §4.8). All present filters must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmittedJobListFilter {
    pub submitter_sid: Option<String>,
    pub identity_sid: Option<String>,
    pub logon_session: Option<u64>,
    pub state: Option<JobState>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SubmittedJobStore {
    entries: BTreeMap<JobId, SubmittedJobEntry>,
}

impl SubmittedJobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: JobId) -> Option<&SubmittedJobEntry> {
        self.entries.get(&id)
    }

    pub fn get_mut(&mut self, id: JobId) -> Option<&mut SubmittedJobEntry> {
        self.entries.get_mut(&id)
    }

    pub fn ids(&self) -> Vec<JobId> {
        self.entries.keys().copied().collect()
    }

    pub fn entries(&self) -> impl Iterator<Item = &SubmittedJobEntry> {
        self.entries.values()
    }

    pub fn live_ids(&self) -> Vec<JobId> {
        self.entries
            .values()
            .filter(|entry| entry.is_live())
            .map(|entry| entry.job_id)
            .collect()
    }

    pub fn live_count_for_submitter(&self, submitter_sid: &str) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.is_live() && entry.submitter_sid == submitter_sid)
            .count()
    }

    pub fn insert(&mut self, entry: SubmittedJobEntry) -> Result<(), SubmittedJobStoreError> {
        if self.entries.contains_key(&entry.job_id) {
            return Err(SubmittedJobStoreError::DuplicateJobId { id: entry.job_id });
        }
        self.entries.insert(entry.job_id, entry);
        Ok(())
    }

    /// Apply the job's terminal event: record the outcome and start the
    /// retention clock. The cause peinit recorded, if any, stays; a setup
    /// failure that never had a cause gets one from the record.
    pub fn record_terminal(
        &mut self,
        event: &JobEvent,
        retention_ns: u64,
    ) -> Result<&SubmittedJobEntry, SubmittedJobStoreError> {
        let entry = self
            .entries
            .get_mut(&event.job_id)
            .ok_or(SubmittedJobStoreError::UnknownJob { id: event.job_id })?;
        if entry.outcome.is_some() {
            return Err(SubmittedJobStoreError::AlreadyTerminal { id: event.job_id });
        }
        let JobEventDetail::Ended {
            ended_at_ns,
            exit_code,
            exit_signal,
            failure_cause,
            ..
        } = &event.detail
        else {
            return Err(SubmittedJobStoreError::NotTerminal { id: event.job_id });
        };
        if entry.cause.is_none() {
            entry.cause = cause_from_terminal_record(event.state, failure_cause.as_deref());
        }
        entry.stop = None;
        entry.outcome = Some(SubmittedJobOutcome {
            state: event.state,
            pid: event.pid,
            started_at_ns: event.started_at_ns,
            ended_at_ns: *ended_at_ns,
            exit_code: *exit_code,
            exit_signal: *exit_signal,
            failure_cause: failure_cause.clone(),
            retained_until_ns: ended_at_ns.saturating_add(retention_ns),
        });
        Ok(entry)
    }

    /// Begin a stop: record the cause and arm the kill deadline. A stop
    /// already in progress is left alone, as §7.8 requires.
    pub fn begin_stop(
        &mut self,
        id: JobId,
        cause: SubmittedJobCause,
        now_ns: u64,
    ) -> Result<bool, SubmittedJobStoreError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(SubmittedJobStoreError::UnknownJob { id })?;
        if entry.outcome.is_some() || entry.stop.is_some() {
            return Ok(false);
        }
        entry.cause = Some(cause);
        entry.stop = Some(SubmittedJobStop {
            cause,
            requested_at_ns: now_ns,
            phase: SubmittedJobStopPhase::Terminating,
            due_at_ns: now_ns.saturating_add(entry.definition.stop_timeout_secs * NANOS_PER_SEC),
        });
        Ok(true)
    }

    pub fn record_kill(
        &mut self,
        id: JobId,
        now_ns: u64,
        post_kill_timeout_secs: u64,
    ) -> Result<(), SubmittedJobStoreError> {
        let entry = self
            .entries
            .get_mut(&id)
            .ok_or(SubmittedJobStoreError::UnknownJob { id })?;
        if let Some(stop) = entry.stop.as_mut() {
            stop.phase = SubmittedJobStopPhase::Killed;
            stop.due_at_ns = now_ns.saturating_add(post_kill_timeout_secs * NANOS_PER_SEC);
        }
        Ok(())
    }

    /// The earliest deadline held against any submitted job, with what it is.
    pub fn next_deadline(&self, started_at_by_job: impl Fn(JobId) -> Option<u64>) -> Option<SubmittedJobDeadline> {
        self.entries
            .values()
            .flat_map(|entry| entry_deadlines(entry, started_at_by_job(entry.job_id)))
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.job_id))
    }

    pub fn due_deadlines(
        &self,
        now_ns: u64,
        started_at_by_job: impl Fn(JobId) -> Option<u64>,
    ) -> Vec<SubmittedJobDeadline> {
        let mut due: Vec<_> = self
            .entries
            .values()
            .flat_map(|entry| entry_deadlines(entry, started_at_by_job(entry.job_id)))
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .collect();
        due.sort_by_key(|deadline| (deadline.due_at_ns, deadline.job_id));
        due
    }

    pub fn next_retention_deadline_ns(&self) -> Option<u64> {
        self.entries
            .values()
            .filter_map(|entry| entry.outcome.as_ref())
            .map(|outcome| outcome.retained_until_ns)
            .min()
    }

    /// Drop terminal entries whose grace period has elapsed and whose
    /// cgroup no longer needs attention.
    pub fn purge_retained_until(&mut self, now_ns: u64) -> Vec<JobId> {
        let purged: Vec<JobId> = self
            .entries
            .values()
            .filter(|entry| {
                entry
                    .outcome
                    .as_ref()
                    .is_some_and(|outcome| outcome.retained_until_ns <= now_ns)
                    && entry.cgroup_cleanup_due_at_ns.is_none()
            })
            .map(|entry| entry.job_id)
            .collect();
        for id in &purged {
            self.entries.remove(id);
        }
        purged
    }

    pub fn remove(&mut self, id: JobId) -> Option<SubmittedJobEntry> {
        self.entries.remove(&id)
    }

    pub fn filtered_ids(&self, filter: &SubmittedJobListFilter, state_of: impl Fn(&SubmittedJobEntry) -> JobState) -> Vec<JobId> {
        self.entries
            .values()
            .filter(|entry| {
                filter
                    .submitter_sid
                    .as_deref()
                    .is_none_or(|sid| entry.submitter_sid == sid)
                    && filter
                        .identity_sid
                        .as_deref()
                        .is_none_or(|sid| entry.identity.user_sid == sid)
                    && filter
                        .logon_session
                        .is_none_or(|session| entry.identity.logon_session == session)
                    && filter.state.is_none_or(|state| state_of(entry) == state)
            })
            .map(|entry| entry.job_id)
            .collect()
    }
}

/// The default retention, as a convenience for callers that do not
/// configure one.
pub fn default_retention_ns() -> u64 {
    DEFAULT_SUBMITTED_JOB_RETENTION_NS
}

fn entry_deadlines(entry: &SubmittedJobEntry, started_at_ns: Option<u64>) -> Vec<SubmittedJobDeadline> {
    let mut deadlines = Vec::new();
    if let Some(due_at_ns) = entry.cgroup_cleanup_due_at_ns {
        deadlines.push(SubmittedJobDeadline {
            job_id: entry.job_id,
            kind: SubmittedJobDeadlineKind::CgroupCleanup,
            due_at_ns,
        });
    }
    if entry.outcome.is_some() {
        return deadlines;
    }
    if let Some(stop) = &entry.stop {
        deadlines.push(SubmittedJobDeadline {
            job_id: entry.job_id,
            kind: match stop.phase {
                SubmittedJobStopPhase::Terminating => SubmittedJobDeadlineKind::StopKill,
                SubmittedJobStopPhase::Killed => SubmittedJobDeadlineKind::PostKill,
            },
            due_at_ns: stop.due_at_ns,
        });
        // A job being stopped has no other deadline worth acting on.
        return deadlines;
    }
    let Some(started_at_ns) = started_at_ns else {
        return deadlines;
    };
    if entry.definition.timeout_secs > 0 {
        deadlines.push(SubmittedJobDeadline {
            job_id: entry.job_id,
            kind: SubmittedJobDeadlineKind::Timeout,
            due_at_ns: started_at_ns
                .saturating_add(entry.definition.timeout_secs * NANOS_PER_SEC),
        });
    }
    if entry.definition.readiness == JobReadiness::Notify && entry.ready == Some(false) {
        deadlines.push(SubmittedJobDeadline {
            job_id: entry.job_id,
            kind: SubmittedJobDeadlineKind::ReadinessTimeout,
            due_at_ns: started_at_ns
                .saturating_add(entry.definition.readiness_timeout_secs * NANOS_PER_SEC),
        });
    }
    deadlines
}

fn cause_from_terminal_record(
    state: JobState,
    failure_cause: Option<&str>,
) -> Option<SubmittedJobCause> {
    match state {
        JobState::Abandoned => Some(SubmittedJobCause::ProcessUnkillable),
        JobState::Failed => failure_cause.and_then(|cause| {
            if cause.starts_with("ParentSetupFailure") {
                Some(SubmittedJobCause::ParentSetupFailure)
            } else if cause.starts_with("PreExecFailure") {
                Some(SubmittedJobCause::PreExecFailure)
            } else {
                None
            }
        }),
        JobState::Created | JobState::Running | JobState::Completed => None,
    }
}
