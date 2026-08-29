use crate::boundary::{BoundaryError, LaunchedProcess};
use crate::ids::JobId;
use crate::job::{JobEvent, JobState, JobStoreError};
use crate::security::TokenSummary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCreatedJobRequest {
    pub job_id: JobId,
    pub launched_at_ns: u64,
    pub notify_socket_path: String,
    pub setup_timeout_secs: u64,
    pub output_pipe_buffer_bytes: usize,
    pub token_source: LaunchTokenSource,
}

/// Where the token a launch installs comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchTokenSource {
    /// Materialised from the job's service identity (§4.1).
    #[default]
    ServiceIdentity,
    /// A primary token the submission path already prepared; the launch
    /// takes a copy and the caller closes the original afterwards.
    Prepared { token_fd: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCreatedJobDispatch {
    pub job_id: JobId,
    pub process: LaunchedProcess,
    pub job_event: JobEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchCreatedJobResult {
    Started(Box<LaunchCreatedJobDispatch>),
    PendingSetup(PendingLaunchSetup),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLaunchSetup {
    pub job_id: JobId,
    pub process: LaunchedProcess,
    pub token_summary: TokenSummary,
    pub launched_at_ns: u64,
    pub setup_deadline_ns: u64,
}

impl PendingLaunchSetup {
    pub fn setup_status_fd(&self) -> Option<i32> {
        self.process.setup_status_fd
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchCreatedJobError {
    JobStore(JobStoreError),
    NotServiceMainJob {
        job_id: JobId,
    },
    NotReloadHookJob {
        job_id: JobId,
        job_type: crate::job::JobType,
    },
    NotPreExecHookJob {
        job_id: JobId,
        job_type: crate::job::JobType,
    },
    NotPostExecHookJob {
        job_id: JobId,
        job_type: crate::job::JobType,
    },
    NotHealthCheckJob {
        job_id: JobId,
        job_type: crate::job::JobType,
    },
    NotSubmittedJob {
        job_id: JobId,
        job_type: crate::job::JobType,
    },
    ServiceMainJobMissingService {
        job_id: JobId,
    },
    JobNotCreated {
        job_id: JobId,
        state: JobState,
    },
    StartBeforeCreation {
        job_id: JobId,
        created_at_ns: u64,
        launched_at_ns: u64,
    },
    Boundary(BoundaryError),
}
