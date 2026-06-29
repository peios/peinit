use crate::ids::{JobId, OperationId};
use crate::security::TokenSummary;
use crate::service::ServiceEnvironmentVariable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobType {
    ServiceMain,
    PreExecHook,
    PostExecHook,
    ReloadHook,
    HealthCheck,
    AdHoc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Created,
    Running,
    Completed,
    Failed,
    Abandoned,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Abandoned)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessHandle {
    pub pid: u32,
    pub pidfd: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobExit {
    ExitCode(i32),
    Signal(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub id: JobId,
    pub service: Option<String>,
    pub job_type: JobType,
    pub hook_index: Option<usize>,
    pub state: JobState,
    pub pid: Option<u32>,
    pub pidfd: Option<i32>,
    pub resolved_identity: String,
    pub token_summary: TokenSummary,
    pub required_privileges: Vec<String>,
    pub image_path: String,
    pub arguments: Vec<String>,
    pub environment: Vec<ServiceEnvironmentVariable>,
    pub working_directory: String,
    pub limit_nofile: Option<u64>,
    pub limit_core: Option<u64>,
    pub oom_score_adj: i32,
    pub created_at_ns: u64,
    pub started_at_ns: Option<u64>,
    pub ended_at_ns: Option<u64>,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<i32>,
    pub failure_cause: Option<String>,
    pub cgroup_id: String,
    pub activation_generation: u64,
    pub cgroup_generation: u64,
    pub operation_id: Option<OperationId>,
    /// Launch this job's process with its stdio attached to `/dev/console`
    /// instead of the daemon default. Carried from the service definition's
    /// `attach_console`; the compiled-in console service is the only setter.
    pub attach_console: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobTransitionError {
    InvalidTransition {
        id: JobId,
        from: JobState,
        action: JobTransitionAction,
    },
    StartBeforeCreation {
        id: JobId,
        created_at_ns: u64,
        started_at_ns: u64,
    },
    EndBeforeCreation {
        id: JobId,
        created_at_ns: u64,
        ended_at_ns: u64,
    },
    EndBeforeStart {
        id: JobId,
        started_at_ns: u64,
        ended_at_ns: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobTransitionAction {
    Start,
    Complete,
    FailBeforeStart,
    FailRunning,
    Abandon,
}
