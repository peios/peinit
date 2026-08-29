use serde_json::Map;

/// The five commands of PSPU §7.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobsCommand {
    Submit,
    Status,
    Wait,
    Stop,
    Signal,
}

impl JobsCommand {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "submit" => Some(Self::Submit),
            "status" => Some(Self::Status),
            "wait" => Some(Self::Wait),
            "stop" => Some(Self::Stop),
            "signal" => Some(Self::Signal),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::Status => "status",
            Self::Wait => "wait",
            Self::Stop => "stop",
            Self::Signal => "signal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobsWaitCondition {
    Terminal,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedJobsRequest {
    pub command: JobsCommand,
    /// Present for every command but `submit`, as the raw identifier text.
    pub job_id: Option<String>,
    /// For `stop`: whether to block until terminal. Defaults to true.
    pub wait: bool,
    /// For `wait`: what to wait for. Defaults to terminal.
    pub wait_for: JobsWaitCondition,
    /// For `signal`: the signal number.
    pub signal: Option<i32>,
    /// For `submit`: the whole request object, for the definition parser.
    pub submit: Option<Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobsRequestParseError {
    MalformedRequest,
    InvalidCommand,
    InvalidArguments,
}

/// The closed error vocabulary of PSPU §7.10.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobsErrorCode {
    MalformedRequest,
    RequestTooLarge,
    InvalidCommand,
    InvalidArguments,
    UnknownJob,
    AccessDenied,
    InvalidState,
    QuotaExceeded,
    BadToken,
    InternalError,
}

impl JobsErrorCode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "MALFORMED_REQUEST" => Some(Self::MalformedRequest),
            "REQUEST_TOO_LARGE" => Some(Self::RequestTooLarge),
            "INVALID_COMMAND" => Some(Self::InvalidCommand),
            "INVALID_ARGUMENTS" => Some(Self::InvalidArguments),
            "UNKNOWN_JOB" => Some(Self::UnknownJob),
            "ACCESS_DENIED" => Some(Self::AccessDenied),
            "INVALID_STATE" => Some(Self::InvalidState),
            "QUOTA_EXCEEDED" => Some(Self::QuotaExceeded),
            "BAD_TOKEN" => Some(Self::BadToken),
            "INTERNAL_ERROR" => Some(Self::InternalError),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MalformedRequest => "MALFORMED_REQUEST",
            Self::RequestTooLarge => "REQUEST_TOO_LARGE",
            Self::InvalidCommand => "INVALID_COMMAND",
            Self::InvalidArguments => "INVALID_ARGUMENTS",
            Self::UnknownJob => "UNKNOWN_JOB",
            Self::AccessDenied => "ACCESS_DENIED",
            Self::InvalidState => "INVALID_STATE",
            Self::QuotaExceeded => "QUOTA_EXCEEDED",
            Self::BadToken => "BAD_TOKEN",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    /// Whether the manager closes the connection after this error (§7.4).
    pub fn closes_connection(self) -> bool {
        matches!(self, Self::RequestTooLarge)
    }
}

impl From<JobsRequestParseError> for JobsErrorCode {
    fn from(error: JobsRequestParseError) -> Self {
        match error {
            JobsRequestParseError::MalformedRequest => Self::MalformedRequest,
            JobsRequestParseError::InvalidCommand => Self::InvalidCommand,
            JobsRequestParseError::InvalidArguments => Self::InvalidArguments,
        }
    }
}
