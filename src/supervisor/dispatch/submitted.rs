use crate::execution::launch::LaunchCreatedJobDispatch;
use crate::ids::JobId;
use crate::job::JobEvent;
use crate::submitted::{
    JobAccessDenied, JobProgress, JobProgressUnit, SubmittedJobCause, SubmittedNotifyField,
};

use super::launch::SupervisorPendingProcessSetupDispatch;

/// A `submit` was accepted: the record exists and the launch is queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorJobSubmitDispatch {
    pub job_event: JobEvent,
    pub submitter_sid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorSubmittedLaunchResult {
    Launched(SupervisorSubmittedLaunchDispatch),
    Failed(SupervisorSubmittedLaunchFailureDispatch),
    PendingSetup(SupervisorPendingProcessSetupDispatch),
}

/// A submitted job's process is running; the runtime registers its pipes
/// and adopts its output sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSubmittedLaunchDispatch {
    pub launch: LaunchCreatedJobDispatch,
    /// The submitter's output sink, for the runtime to write the job's
    /// lines to. Raw; the runtime owns it from here.
    pub output_sink_fd: Option<i32>,
}

/// A submitted job could not be started; the record is terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSubmittedLaunchFailureDispatch {
    pub job_event: JobEvent,
    pub cause: SubmittedJobCause,
}

/// A submitted job's process ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisedSubmittedTerminalDispatch {
    pub job_event: JobEvent,
    pub cause: Option<SubmittedJobCause>,
    /// The job's cgroup could not be removed yet; a retry is scheduled.
    pub cgroup_busy: bool,
}

/// peinit signalled a submitted job to stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSubmittedStopDispatch {
    pub job_id: JobId,
    pub cause: SubmittedJobCause,
    /// False when the job had already sent `STOPPING=1`.
    pub signalled: bool,
}

/// What a due submitted-job deadline did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorSubmittedDeadlineDispatch {
    /// A timeout or readiness timeout began a stop.
    Stop(SupervisorSubmittedStopDispatch),
    /// The termination grace elapsed and the cgroup was killed.
    Killed { job_id: JobId },
    /// The post-kill grace elapsed with the cgroup still populated.
    Abandoned {
        job_event: Box<JobEvent>,
        cgroup_id: String,
    },
    /// A busy cgroup was retried.
    CgroupCleanup { job_id: JobId, leaked: bool },
}

/// A notification a submitted job sent, applied to its entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSubmittedNotifyDispatch {
    pub job_id: JobId,
    pub submitter_sid: String,
    pub applied: Vec<SubmittedNotifyField>,
    /// Whether this datagram is due a `job.status` event under the
    /// per-job rate bound (PSPU §4.19).
    pub status_event_due: bool,
    /// The job's retained values after this datagram, for the event.
    pub status_text: Option<String>,
    pub progress: Option<JobProgress>,
    pub progress_unit: Option<JobProgressUnit>,
}

/// A command on the jobs channel that changed something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorJobsCommandDispatch {
    Submit(SupervisorJobSubmitDispatch),
    Stop(SupervisorSubmittedStopDispatch),
    /// A stop reached a job that had not launched: it is terminal without
    /// ever having run.
    Cancelled(SupervisorSubmittedLaunchFailureDispatch),
    Signal {
        job_id: JobId,
        signal: i32,
    },
}

/// An access check on a job's descriptor denied the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorJobAccessDeniedDispatch {
    pub denied: JobAccessDenied,
}
