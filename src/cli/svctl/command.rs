use std::path::PathBuf;

use crate::shutdown::ShutdownKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub socket_path: PathBuf,
    pub jobs_socket_path: PathBuf,
    pub output: OutputMode,
    pub command: Command,
}

/// Which of peinit's two doors a command knocks on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Control,
    Jobs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Service {
        action: ServiceAction,
        service: String,
        wait: bool,
    },
    Status {
        service: String,
    },
    List,
    OperationStatus {
        operation_id: String,
    },
    ReloadConfig,
    Shutdown {
        kind: ShutdownKind,
    },
    /// `job-list` on the control socket.
    JobList {
        filter: JobListFilter,
    },
    /// `job-status` on the control socket.
    JobStatus {
        job_id: String,
    },
    /// `job-stop` on the control socket.
    JobStop {
        job_id: String,
        wait: bool,
    },
    /// `submit` on the jobs socket, as the caller's own primary token.
    JobSubmit {
        submission: JobSubmission,
        wait: bool,
    },
    /// `wait` on the jobs socket.
    JobWait {
        job_id: String,
        for_ready: bool,
    },
    /// `signal` on the jobs socket.
    JobSignal {
        job_id: String,
        signal: i32,
    },
}

impl Command {
    pub fn channel(&self) -> Channel {
        match self {
            Self::JobSubmit { .. } | Self::JobWait { .. } | Self::JobSignal { .. } => Channel::Jobs,
            _ => Channel::Control,
        }
    }
}

/// The `job-list` filters (PSPU §4.8), each optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobListFilter {
    pub submitter: Option<String>,
    pub identity: Option<String>,
    pub logon_session: Option<u64>,
    pub state: Option<String>,
}

/// A job definition as the command line states it (PSPU §7.6), plus the
/// descriptors of this process to attach.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobSubmission {
    pub image_path: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub working_directory: Option<String>,
    pub description: Option<String>,
    pub timeout_secs: Option<u64>,
    pub stop_timeout_secs: Option<u64>,
    pub readiness: Option<String>,
    pub readiness_timeout_secs: Option<u64>,
    pub success_exit_codes: Option<Vec<i32>>,
    /// `(name, fd)`: a descriptor of this process passed under `name`.
    pub descriptors: Vec<(String, i32)>,
    /// Attach this process's standard output as the job's output sink.
    pub output: bool,
    pub security_descriptor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Reload,
    Reset,
}

impl ServiceAction {
    pub fn command_name(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Reload => "reload",
            Self::Reset => "reset",
        }
    }

    pub fn default_wait(self) -> bool {
        matches!(self, Self::Start | Self::Stop | Self::Restart)
    }

    pub fn accepts_wait(self) -> bool {
        !matches!(self, Self::Reset)
    }
}
