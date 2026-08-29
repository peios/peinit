use std::borrow::Cow;

use crate::ids::JobId;
use crate::jobs::wire::{JobsErrorCode, JobsRequestParseError, jobs_error_response_message};
use crate::submitted::{JobAccessDenied, SubmittedJobDefinitionError};

/// Why a jobs-channel command was refused, with the code PSPU §7.10 gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobsCommandError {
    Parse(JobsRequestParseError),
    /// The content did not fit the receive buffer.
    Truncated,
    /// The attachments did not fit; the request does not describe the job
    /// the submitter meant.
    ControlTruncated,
    Definition(SubmittedJobDefinitionError),
    InvalidJobId,
    UnknownJob {
        job_id: JobId,
    },
    AccessDenied(Box<JobAccessDenied>),
    InvalidState {
        job_id: JobId,
        reason: &'static str,
    },
    QuotaExceeded {
        submitter_sid: String,
        live: usize,
        limit: usize,
    },
    BadToken(String),
    InvalidDescriptor(String),
    ShuttingDown,
    Internal(String),
}

impl JobsCommandError {
    pub fn code(&self) -> JobsErrorCode {
        match self {
            Self::Parse(error) => JobsErrorCode::from(*error),
            Self::Truncated => JobsErrorCode::RequestTooLarge,
            Self::ControlTruncated
            | Self::Definition(_)
            | Self::InvalidJobId
            | Self::InvalidDescriptor(_) => JobsErrorCode::InvalidArguments,
            Self::UnknownJob { .. } => JobsErrorCode::UnknownJob,
            Self::AccessDenied(_) => JobsErrorCode::AccessDenied,
            Self::InvalidState { .. } | Self::ShuttingDown => JobsErrorCode::InvalidState,
            Self::QuotaExceeded { .. } => JobsErrorCode::QuotaExceeded,
            Self::BadToken(_) => JobsErrorCode::BadToken,
            Self::Internal(_) => JobsErrorCode::InternalError,
        }
    }

    pub fn message(&self) -> Cow<'static, str> {
        match self {
            Self::Parse(_) | Self::Truncated | Self::Internal(_) => {
                Cow::Borrowed(jobs_error_response_message(self.code()))
            }
            Self::ControlTruncated => {
                Cow::Borrowed("attachments exceeded what the manager accepts on one message")
            }
            Self::Definition(error) => Cow::Owned(error.message()),
            Self::InvalidJobId => Cow::Borrowed("job_id is not a well-formed identifier"),
            Self::InvalidDescriptor(reason) => {
                Cow::Owned(format!("invalid security_descriptor: {reason}"))
            }
            Self::UnknownJob { job_id } => Cow::Owned(format!("unknown job {job_id}")),
            Self::AccessDenied(denied) => Cow::Owned(format!(
                "caller lacks {} on job {}",
                denied.desired_access.label(),
                denied.job_id
            )),
            Self::InvalidState { job_id, reason } => Cow::Owned(format!("job {job_id}: {reason}")),
            Self::QuotaExceeded {
                submitter_sid,
                live,
                limit,
            } => Cow::Owned(format!(
                "submitter {submitter_sid} holds {live} live jobs of a limit of {limit}"
            )),
            Self::BadToken(reason) => Cow::Owned(format!("attached token: {reason}")),
            Self::ShuttingDown => Cow::Borrowed("submissions are refused during shutdown"),
        }
    }

    pub fn closes_connection(&self) -> bool {
        self.code().closes_connection()
    }
}
