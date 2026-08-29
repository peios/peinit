use crate::shutdown::ShutdownKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedControlRequest {
    pub command: ControlCommand,
    pub service: Option<String>,
    pub wait: bool,
    pub shutdown_kind: Option<ShutdownKind>,
    pub operation_id: Option<String>,
    /// `job_id` for `job-status` and `job-stop`.
    pub job_id: Option<String>,
    /// The `job-list` filters.
    pub job_filter: Option<crate::submitted::SubmittedJobListFilter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlCommand {
    Start,
    Stop,
    Restart,
    Reload,
    Reset,
    Status,
    List,
    Shutdown,
    ReloadConfig,
    OperationStatus,
    JobList,
    JobStatus,
    JobStop,
}

impl ControlCommand {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "start" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            "restart" => Some(Self::Restart),
            "reload" => Some(Self::Reload),
            "reset" => Some(Self::Reset),
            "status" => Some(Self::Status),
            "list" => Some(Self::List),
            "shutdown" => Some(Self::Shutdown),
            "reload-config" => Some(Self::ReloadConfig),
            "operation-status" => Some(Self::OperationStatus),
            "job-list" => Some(Self::JobList),
            "job-status" => Some(Self::JobStatus),
            "job-stop" => Some(Self::JobStop),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Reload => "reload",
            Self::Reset => "reset",
            Self::Status => "status",
            Self::List => "list",
            Self::Shutdown => "shutdown",
            Self::ReloadConfig => "reload-config",
            Self::OperationStatus => "operation-status",
            Self::JobList => "job-list",
            Self::JobStatus => "job-status",
            Self::JobStop => "job-stop",
        }
    }

    pub fn default_wait(self) -> bool {
        matches!(
            self,
            Self::Start | Self::Stop | Self::Restart | Self::JobStop
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlRequestParseError {
    MalformedRequest,
    InvalidCommand,
    InvalidArguments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlErrorCode {
    AccessDenied,
    UnknownService,
    UnknownOperation,
    UnknownJob,
    MalformedRequest,
    RequestTooLarge,
    InvalidCommand,
    InvalidArguments,
    InvalidState,
    OperationTimeout,
    InternalError,
}

impl ControlErrorCode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ACCESS_DENIED" => Some(Self::AccessDenied),
            "UNKNOWN_SERVICE" => Some(Self::UnknownService),
            "UNKNOWN_OPERATION" => Some(Self::UnknownOperation),
            "UNKNOWN_JOB" => Some(Self::UnknownJob),
            "MALFORMED_REQUEST" => Some(Self::MalformedRequest),
            "REQUEST_TOO_LARGE" => Some(Self::RequestTooLarge),
            "INVALID_COMMAND" => Some(Self::InvalidCommand),
            "INVALID_ARGUMENTS" => Some(Self::InvalidArguments),
            "INVALID_STATE" => Some(Self::InvalidState),
            "OPERATION_TIMEOUT" => Some(Self::OperationTimeout),
            "INTERNAL_ERROR" => Some(Self::InternalError),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AccessDenied => "ACCESS_DENIED",
            Self::UnknownService => "UNKNOWN_SERVICE",
            Self::UnknownOperation => "UNKNOWN_OPERATION",
            Self::UnknownJob => "UNKNOWN_JOB",
            Self::MalformedRequest => "MALFORMED_REQUEST",
            Self::RequestTooLarge => "REQUEST_TOO_LARGE",
            Self::InvalidCommand => "INVALID_COMMAND",
            Self::InvalidArguments => "INVALID_ARGUMENTS",
            Self::InvalidState => "INVALID_STATE",
            Self::OperationTimeout => "OPERATION_TIMEOUT",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }
}

impl From<ControlRequestParseError> for ControlErrorCode {
    fn from(error: ControlRequestParseError) -> Self {
        match error {
            ControlRequestParseError::MalformedRequest => Self::MalformedRequest,
            ControlRequestParseError::InvalidCommand => Self::InvalidCommand,
            ControlRequestParseError::InvalidArguments => Self::InvalidArguments,
        }
    }
}

impl From<ControlFrameRejectReason> for ControlErrorCode {
    fn from(reason: ControlFrameRejectReason) -> Self {
        match reason {
            ControlFrameRejectReason::MalformedRequest => Self::MalformedRequest,
            ControlFrameRejectReason::RequestTooLarge => Self::RequestTooLarge,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlResponseStatus {
    Ok,
    Error,
}

impl ControlResponseStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ok" => Some(Self::Ok),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFrameDecision {
    Complete { body: Vec<u8>, consumed: usize },
    Incomplete,
    Reject { reason: ControlFrameRejectReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFrameRejectReason {
    MalformedRequest,
    RequestTooLarge,
}
