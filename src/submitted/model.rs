use crate::ids::JobId;
use crate::job::{JobRecord, JobState};

use super::definition::SubmittedJobDefinition;
use super::security::JobSecurityDescriptor;

/// How long a terminal submitted job's record is held for a submitter that is
/// polling for the outcome. The operation retention value, for the same
/// reason (PSPU §7.A).
pub const DEFAULT_SUBMITTED_JOB_RETENTION_NS: u64 = 60_000_000_000;

/// The identity a submitted job runs as, as the kernel reported it from the
/// primary token peinit installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobIdentity {
    pub user_sid: String,
    pub logon_session: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobReadiness {
    None,
    Notify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobProgressUnit {
    Bytes,
    Items,
    Percent,
}

impl JobProgressUnit {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Items => "items",
            Self::Percent => "percent",
        }
    }
}

/// The most recent accepted `PROGRESS=` value (PSPU §4.19).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobProgress {
    pub current: u64,
    /// `Some` for `N/T`; `None` for `N` and `N/`.
    pub total: Option<u64>,
    /// True for `N/` and `N/T`: an end exists, whether or not it is known.
    pub bounded: bool,
}

/// Why peinit brought a submitted job to its end, where peinit decided it
/// (PSPU §7.7). `None` on the entry means the process ended of its own accord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmittedJobCause {
    ParentSetupFailure,
    PreExecFailure,
    ReadinessTimeout,
    Timeout,
    ExplicitStop,
    Shutdown,
    ProcessUnkillable,
}

impl SubmittedJobCause {
    pub fn wire(self) -> &'static str {
        match self {
            Self::ParentSetupFailure => "parent_setup_failure",
            Self::PreExecFailure => "pre_exec_failure",
            Self::ReadinessTimeout => "readiness_timeout",
            Self::Timeout => "timeout",
            Self::ExplicitStop => "explicit_stop",
            Self::Shutdown => "shutdown",
            Self::ProcessUnkillable => "process_unkillable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmittedJobStopPhase {
    /// The termination signal has been sent; the kill is due at `kill_due_at_ns`.
    Terminating,
    /// The cgroup has been killed; the post-kill check is due at `kill_due_at_ns`.
    Killed,
}

/// A stop in progress against a running submitted job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedJobStop {
    pub cause: SubmittedJobCause,
    pub requested_at_ns: u64,
    pub phase: SubmittedJobStopPhase,
    pub due_at_ns: u64,
}

/// What is left of a submitted job once it is terminal: the facts the job
/// view needs, held past the `JobStore` dropping the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedJobOutcome {
    pub state: JobState,
    pub pid: Option<u32>,
    pub started_at_ns: Option<u64>,
    pub ended_at_ns: u64,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<i32>,
    pub failure_cause: Option<String>,
    pub retained_until_ns: u64,
}

/// A notification field a submitted job may have applied, for the event
/// stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmittedNotifyField {
    Ready,
    Status { text: String },
    Progress { progress: JobProgress },
    ProgressUnit { unit: JobProgressUnit },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedJobEntry {
    pub job_id: JobId,
    /// The connection identity that submitted the job: the owner of its
    /// descriptor and the principal its quota counts against.
    pub submitter_sid: String,
    pub identity: JobIdentity,
    pub definition: SubmittedJobDefinition,
    pub security_descriptor: JobSecurityDescriptor,
    pub created_at_ns: u64,
    /// The primary token to install on the job, held between submission
    /// and launch. Raw, as every descriptor in the job model is; the launch
    /// takes it and closes it.
    pub prepared_token_fd: Option<i32>,
    /// Descriptors attached to the submit, to be injected from 3 upward in
    /// this order, paired with their `LISTEN_FDNAMES` names. Raw until the
    /// launch takes them.
    pub attached_descriptors: Vec<(String, i32)>,
    /// The output sink the submitter attached, until the runtime adopts it.
    pub output_sink_fd: Option<i32>,
    /// `Some(false)` while a `Notify` job has not sent `READY=1`; `None` for
    /// a job with no readiness protocol.
    pub ready: Option<bool>,
    pub status_text: Option<String>,
    pub progress: Option<JobProgress>,
    pub progress_unit: Option<JobProgressUnit>,
    /// When the last `job.status` event was emitted for this job, for the
    /// per-job rate bound.
    pub last_status_event_ns: Option<u64>,
    /// The job sent `STOPPING=1`: a stop sends no termination signal.
    pub stopping_acknowledged: bool,
    /// The cause peinit recorded when it decided the job's end. `None` while
    /// the job runs undisturbed, and afterwards if the process ended of its
    /// own accord.
    pub cause: Option<SubmittedJobCause>,
    pub stop: Option<SubmittedJobStop>,
    pub outcome: Option<SubmittedJobOutcome>,
    /// The job's cgroup, retained after the record is dropped so a busy
    /// cgroup can be retried and reported.
    pub cgroup_id: String,
    /// A cgroup that could not be removed after the job ended is retried
    /// once at this deadline, then reported as leaked.
    pub cgroup_cleanup_due_at_ns: Option<u64>,
    /// Whether an `output.dropped` event has already been emitted.
    pub output_drop_reported: bool,
}

impl SubmittedJobEntry {
    pub fn is_terminal(&self) -> bool {
        self.outcome.is_some()
    }

    /// Whether the job counts against its submitter's quota.
    pub fn is_live(&self) -> bool {
        self.outcome.is_none()
    }

    /// The identity this entry was created for, as the job record's
    /// `resolved_identity` carries it.
    pub fn matches_record(&self, record: &JobRecord) -> bool {
        record.id == self.job_id
    }
}
